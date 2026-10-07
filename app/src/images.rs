//! `oneshot-img` protocol: the WebView requests artwork by opaque reference
//! and size bucket; Rust resolves the authenticated URL, fetches, caches on
//! disk and serves bytes. Tokens never reach the WebView.
//!
//! URL path (built by `ui/src/ipc/images.ts`):
//! `<size>/<kind>/<item-ref>/<tag>` with each segment URI-encoded.
//!
//! A second form serves profile pictures: `avatar/<profile-id>/<key>` (the
//! key only changes the URL when the picture changes).

use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, LazyLock};

use oneshot_core::ids::ItemRef;
use oneshot_core::media::{ImageKind, ImageRef, ImageSize};
use oneshot_core::profile::ProfileId;
use oneshot_core::{Error, Result};
use oneshot_storage::images::cache_key;
use serde::Serialize;
use tauri::http::{Request, Response, StatusCode};
use tauri::{AppHandle, Manager, UriSchemeResponder};

use crate::state::AppState;

fn decode(seg: &str) -> String {
    url::form_urlencoded::parse(format!("x={seg}").as_bytes()).next().map(|(_, v)| v.into_owned()).unwrap_or_default()
}

fn parse_kind(s: &str) -> Option<ImageKind> {
    Some(match s {
        "poster" => ImageKind::Poster,
        "backdrop" => ImageKind::Backdrop,
        "thumb" => ImageKind::Thumb,
        "logo" => ImageKind::Logo,
        "banner" => ImageKind::Banner,
        _ => return None,
    })
}

pub fn parse_path(path: &str) -> Option<(ImageRef, ImageSize)> {
    let mut parts = path.trim_start_matches('/').splitn(4, '/');
    let size = ImageSize::parse(parts.next()?)?;
    let kind = parse_kind(parts.next()?)?;
    let item: ItemRef = decode(parts.next()?).parse().ok()?;
    let tag = decode(parts.next()?);
    Some((ImageRef { item, kind, tag, blurhash: None }, size))
}

/// Images being fetched, by cache key.
type Inflight = parking_lot::Mutex<HashMap<String, Arc<tokio::sync::Mutex<()>>>>;
static INFLIGHT: LazyLock<Inflight> = LazyLock::new(Default::default);

/// The cached bytes under `key`, else `fetch`'s, stored for next time. One
/// fetch per key at a time: the same picture asked twice at once (a hero and
/// a card, the ambience and a shelf) is downloaded once, and the second
/// request is served from the disk cache.
async fn cached_or_fetch(state: &AppState, key: &str, fetch: impl Future<Output = Result<Vec<u8>>>) -> Result<Vec<u8>> {
    if let Some(bytes) = disk_get(state, key).await {
        return Ok(bytes);
    }
    let gate = Arc::clone(INFLIGHT.lock().entry(key.to_owned()).or_default());
    let result = async {
        let _one = gate.lock().await;
        if let Some(bytes) = disk_get(state, key).await {
            return Ok(bytes);
        }
        let bytes = fetch.await?;
        let (images, owned_key, copy) = (state.images.clone(), key.to_owned(), bytes.clone());
        match tokio::task::spawn_blocking(move || images.put(&owned_key, &copy)).await {
            Ok(Ok(())) => {}
            Ok(Err(e)) => tracing::warn!(target: "cache", "image cache write failed: {e}"),
            Err(e) => tracing::warn!(target: "cache", "image cache write failed: {e}"),
        }
        // Trim the cache periodically rather than on every write.
        static WRITES: AtomicUsize = AtomicUsize::new(0);
        if WRITES.fetch_add(1, Ordering::Relaxed) % 250 == 249 {
            let images = state.images.clone();
            tokio::task::spawn_blocking(move || images.enforce_limit());
        }
        Ok(bytes)
    }
    .await;
    // Last one out removes the gate (the map holds one reference, we hold one).
    let mut inflight = INFLIGHT.lock();
    if Arc::strong_count(&gate) <= 2 {
        inflight.remove(key);
    }
    result
}

/// A cache read on the blocking pool: a grid asks for dozens of pictures at
/// once, and file opens can be slow (antivirus scanning on Windows); the
/// async workers stay free for IPC commands meanwhile.
async fn disk_get(state: &AppState, key: &str) -> Option<Vec<u8>> {
    let (images, key) = (state.images.clone(), key.to_owned());
    tokio::task::spawn_blocking(move || images.get(&key)).await.ok().flatten()
}

async fn download(req: oneshot_net::reqwest::RequestBuilder) -> Result<Vec<u8>> {
    let resp = oneshot_net::ensure_ok(req.send().await.map_err(oneshot_net::map_err)?).await?;
    Ok(resp.bytes().await.map_err(oneshot_net::map_err)?.to_vec())
}

/// Fetches (or reads from cache) the image bytes, of a live connection only
/// (the disk cache also holds other profiles' artwork).
pub async fn load(state: &AppState, image: &ImageRef, size: ImageSize) -> Result<Vec<u8>> {
    // A downloaded title keeps its own pictures; there is no server to ask.
    if image.item.server == crate::offline::LOCAL_SERVER {
        return state.downloads.local_image(&image.item.key, image.kind).await;
    }
    let provider = state.catalog.provider(image.item.server)?;
    let key = cache_key(image, size);
    cached_or_fetch(state, &key, async {
        let mut req = state.http().get(provider.image_url(image, size)?);
        for (k, v) in provider.auth_headers() {
            req = req.header(k, v);
        }
        download(req).await
    })
    .await
}

fn mime(bytes: &[u8]) -> &'static str {
    match bytes {
        [0xFF, 0xD8, ..] => "image/jpeg",
        [0x89, b'P', b'N', b'G', ..] => "image/png",
        [b'R', b'I', b'F', b'F', _, _, _, _, b'W', b'E', b'B', b'P', ..] => "image/webp",
        _ => "application/octet-stream",
    }
}

pub fn handle(app: &AppHandle, request: Request<Vec<u8>>, responder: UriSchemeResponder) {
    let app = app.clone();
    let path = request.uri().path().to_owned();
    tauri::async_runtime::spawn(async move {
        let state = app.state::<Arc<AppState>>();
        let p = path.trim_start_matches('/');
        let loaded = if let Some(rest) = p.strip_prefix("tmdb/") {
            Some(load_tmdb(&state, rest).await)
        } else if let Some(rest) = p.strip_prefix("avatar/") {
            Some(load_avatar(&state, rest).await)
        } else {
            match parse_path(&path) {
                Some((image, size)) => Some(load(&state, &image, size).await),
                None => None,
            }
        };
        let response = match loaded {
            None => Response::builder().status(StatusCode::BAD_REQUEST).body(Vec::new()),
            Some(Ok(bytes)) => Response::builder()
                .header("Content-Type", mime(&bytes))
                .header("Cache-Control", "max-age=604800, immutable")
                .body(bytes),
            Some(Err(e)) => {
                tracing::debug!(target: "cache", "image {path}: {e}");
                Response::builder().status(StatusCode::NOT_FOUND).body(Vec::new())
            }
        };
        responder.respond(response.unwrap_or_else(|_| Response::new(Vec::new())));
    });
}

/// A profile's picture: a public URL (plex.tv, a Jellyfin sign-in screen),
/// proxied because the WebView loads no remote origin.
async fn load_avatar(state: &AppState, rest: &str) -> Result<Vec<u8>> {
    let id: ProfileId = rest.split('/').next().unwrap_or_default().parse().map_err(|_| Error::Invalid("avatar path".into()))?;
    let url = state
        .resolved_profiles()
        .iter()
        .find(|r| r.profile.id == id)
        .and_then(oneshot_storage::profiles::avatar_of)
        .ok_or_else(|| Error::NotFound(format!("avatar of profile {id}")))?;
    let key = oneshot_storage::images::avatar_cache_key(&url);
    cached_or_fetch(state, &key, download(state.http().get(url))).await
}

/// A TMDB photo or poster (`tmdb/<size>/<file>`), public; only TMDB files,
/// so the route cannot fetch anything else.
pub async fn load_tmdb(state: &AppState, rest: &str) -> Result<Vec<u8>> {
    let url = oneshot_tmdb::image_url(rest).ok_or_else(|| Error::Invalid("tmdb image path".into()))?;
    let key = oneshot_storage::images::tmdb_cache_key(&url);
    cached_or_fetch(state, &key, download(state.http().get(url))).await
}

/// Colours for the adaptive background, extracted from a tiny rendition.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Palette {
    /// Dominant colours, most prominent first, as `#rrggbb`.
    pub colors: Vec<String>,
    /// A dark, desaturated tone safe behind white text.
    pub base: String,
    /// A brighter accent derived from the most saturated cluster.
    pub accent: String,
}

pub fn palette(bytes: &[u8]) -> Result<Palette> {
    let img = image::load_from_memory(bytes).map_err(|e| Error::Other(format!("image decode: {e}")))?;
    let small = img.thumbnail(48, 48).to_rgb8();
    let pixels: Vec<[f32; 3]> = small.pixels().map(|p| [f32::from(p[0]), f32::from(p[1]), f32::from(p[2])]).collect();
    let clusters = kmeans(&pixels, 5, 8);
    let hex = |c: [f32; 3]| format!("#{:02x}{:02x}{:02x}", c[0] as u8, c[1] as u8, c[2] as u8);
    let dominant = clusters.first().map_or([20.0, 20.0, 24.0], |c| c.0);
    let accent = clusters
        .iter()
        .max_by(|a, b| saturation(a.0).total_cmp(&saturation(b.0)))
        .map_or(dominant, |c| c.0);
    Ok(Palette {
        colors: clusters.iter().map(|c| hex(c.0)).collect(),
        base: hex(tone(dominant, 0.075, 0.45)),
        accent: hex(tone(accent, 0.55, 0.85)),
    })
}

fn saturation(c: [f32; 3]) -> f32 {
    let max = c.iter().copied().fold(0.0, f32::max);
    let min = c.iter().copied().fold(255.0, f32::min);
    if max <= 0.0 { 0.0 } else { (max - min) / max }
}

/// Rescales a colour to a target luminance and saturation factor so text
/// contrast is guaranteed regardless of artwork.
fn tone(c: [f32; 3], luminance: f32, sat: f32) -> [f32; 3] {
    let l = (0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2]) / 255.0;
    let grey = l * 255.0;
    let desat = c.map(|v| grey + (v - grey) * sat);
    let scale = if l > 0.001 { luminance / l } else { 1.0 };
    desat.map(|v| (v * scale).clamp(0.0, 255.0))
}

/// Tiny deterministic k-means; returns (centroid, population) sorted by population.
fn kmeans(pixels: &[[f32; 3]], k: usize, iterations: usize) -> Vec<([f32; 3], usize)> {
    if pixels.is_empty() {
        return Vec::new();
    }
    let mut centroids: Vec<[f32; 3]> = (0..k).map(|i| pixels[i * pixels.len() / k]).collect();
    // `usize::MAX`: not assigned yet, so the first pass always counts as a change.
    let mut assignment = vec![usize::MAX; pixels.len()];
    let mut counts = vec![0usize; k];
    for _ in 0..iterations {
        let mut changed = false;
        for (p, slot) in pixels.iter().zip(assignment.iter_mut()) {
            let nearest = (0..k).min_by(|&a, &b| dist(p, &centroids[a]).total_cmp(&dist(p, &centroids[b]))).unwrap_or(0);
            changed |= *slot != nearest;
            *slot = nearest;
        }
        // Converged: another pass would compute the same centroids.
        if !changed {
            break;
        }
        // One pass over the pixels, in pixel order (same sums as per-cluster passes).
        let mut sums = vec![[0f32; 3]; k];
        counts.fill(0);
        for (p, &c) in pixels.iter().zip(&assignment) {
            for ch in 0..3 {
                sums[c][ch] += p[ch];
            }
            counts[c] += 1;
        }
        for ((centroid, sum), &n) in centroids.iter_mut().zip(&sums).zip(&counts) {
            if n > 0 {
                *centroid = sum.map(|s| s / n as f32);
            }
        }
    }
    counts.fill(0);
    for &c in &assignment {
        counts[c] += 1;
    }
    let mut out: Vec<([f32; 3], usize)> = centroids.into_iter().zip(counts).filter(|(_, n)| *n > 0).collect();
    out.sort_by_key(|c| std::cmp::Reverse(c.1));
    out
}

fn dist(a: &[f32; 3], b: &[f32; 3]) -> f32 {
    (a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)
}

#[cfg(test)]
mod tests {
    use oneshot_core::ServerId;

    use super::*;

    #[test]
    fn parses_encoded_path() {
        let item = ItemRef::new(ServerId::new(), "abc");
        let enc = |s: &str| url::form_urlencoded::byte_serialize(s.as_bytes()).collect::<String>();
        let path = format!("/card/poster/{}/{}", enc(&item.to_string()), enc("/library/metadata/1/thumb/99"));
        let (img, size) = parse_path(&path).unwrap();
        assert_eq!(size, ImageSize::Card);
        assert_eq!(img.item, item);
        assert_eq!(img.tag, "/library/metadata/1/thumb/99");
    }

    #[test]
    fn palette_base_is_dark_enough_for_white_text() {
        let mut img = image::RgbImage::new(16, 16);
        for p in img.pixels_mut() {
            *p = image::Rgb([250, 240, 30]); // very bright yellow artwork
        }
        let mut bytes = Vec::new();
        image::DynamicImage::ImageRgb8(img).write_to(&mut std::io::Cursor::new(&mut bytes), image::ImageFormat::Png).unwrap();
        let p = palette(&bytes).unwrap();
        let v = u32::from_str_radix(&p.base[1..], 16).unwrap();
        let (r, g, b) = ((v >> 16) & 255, (v >> 8) & 255, v & 255);
        let lum = (0.2126 * r as f32 + 0.7152 * g as f32 + 0.0722 * b as f32) / 255.0;
        assert!(lum < 0.1, "base luminance {lum} too bright for white text");
    }
}
