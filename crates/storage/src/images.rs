//! Artwork cache on disk, keyed by (image reference, size bucket).
//!
//! Files are content-addressed by a SHA-256 of the key, so cache paths never
//! leak server URLs or tokens. Eviction is LRU by modification time (touched
//! on read), capped in bytes.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime};

use oneshot_core::media::{ImageRef, ImageSize};
use oneshot_core::{Error, Result};
use sha2::{Digest, Sha256};

/// How recent a file's LRU mark may be before a read refreshes it.
const TOUCH_INTERVAL: Duration = Duration::from_secs(3600);

#[derive(Debug, Clone)]
pub struct ImageCache {
    dir: PathBuf,
    max_bytes: Arc<AtomicU64>,
}

pub fn cache_key(image: &ImageRef, size: ImageSize) -> String {
    let mut h = Sha256::new();
    h.update(image.item.to_string().as_bytes());
    h.update([0]);
    h.update(image.tag.as_bytes());
    h.update([0]);
    h.update(format!("{size:?}").as_bytes());
    h.finalize().iter().map(|b| format!("{b:02x}")).collect()
}

/// Cache key for a profile picture fetched from a public URL.
pub fn avatar_cache_key(url: &url::Url) -> String {
    url_key(b"avatar\0", url)
}

/// Cache key for a TMDB photo or poster.
pub fn tmdb_cache_key(url: &url::Url) -> String {
    url_key(b"tmdb\0", url)
}

fn url_key(prefix: &[u8], url: &url::Url) -> String {
    let mut h = Sha256::new();
    h.update(prefix);
    h.update(url.as_str().as_bytes());
    h.finalize().iter().map(|b| format!("{b:02x}")).collect()
}

fn img_err(e: &dyn std::fmt::Display) -> Error {
    Error::Storage(oneshot_core::codes::STO_IMAGE_CACHE.tag(format!("The picture cache could not be updated ({e}). Check the disk space.")))
}

impl ImageCache {
    pub fn new(dir: PathBuf, max_bytes: u64) -> Result<Self> {
        std::fs::create_dir_all(&dir).map_err(|e| img_err(&e))?;
        Ok(Self { dir, max_bytes: Arc::new(AtomicU64::new(max_bytes)) })
    }

    pub fn set_max_bytes(&self, max_bytes: u64) {
        self.max_bytes.store(max_bytes, Ordering::Relaxed);
    }

    fn path(&self, key: &str) -> PathBuf {
        // Two-level fan-out keeps directories small.
        self.dir.join(&key[..2]).join(key)
    }

    pub fn get(&self, key: &str) -> Option<Vec<u8>> {
        let path = self.path(key);
        // One open serves the read and the LRU touch (read-only fallback).
        let Ok(mut file) = std::fs::File::options().read(true).append(true).open(&path) else {
            return std::fs::read(&path).ok();
        };
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes).ok()?;
        // Touch for LRU, at most once per interval: eviction only needs a
        // rough order, not a metadata write per image shown. Failure only
        // degrades eviction order.
        let now = SystemTime::now();
        let stale = file.metadata().and_then(|m| m.modified()).map_or(true, |m| now.duration_since(m).unwrap_or_default() > TOUCH_INTERVAL);
        if stale {
            let _ = file.set_modified(now);
        }
        Some(bytes)
    }

    pub fn put(&self, key: &str, bytes: &[u8]) -> Result<()> {
        let path = self.path(key);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| img_err(&e))?;
        }
        // A temp name of its own: two writers of the same key (the same
        // picture fetched twice at once) must not interleave in one file.
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let tmp = path.with_extension(format!("{}-{}.tmp", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
        let written = std::fs::write(&tmp, bytes).and_then(|()| std::fs::rename(&tmp, &path));
        if written.is_err() {
            let _ = std::fs::remove_file(&tmp);
        }
        written.map_err(|e| img_err(&e))
    }

    /// Evicts least-recently-used files until the cache fits its budget.
    /// Returns the number of bytes freed.
    pub fn enforce_limit(&self) -> Result<u64> {
        let mut files: Vec<(PathBuf, u64, SystemTime)> = Vec::new();
        collect(&self.dir, &mut files).map_err(|e| img_err(&e))?;
        let max = self.max_bytes.load(Ordering::Relaxed);
        let mut total: u64 = files.iter().map(|f| f.1).sum();
        if total <= max {
            return Ok(0);
        }
        files.sort_by_key(|f| f.2);
        let mut freed = 0;
        for (path, size, _) in files {
            if total <= max * 9 / 10 {
                break;
            }
            if std::fs::remove_file(&path).is_ok() {
                total -= size;
                freed += size;
            }
        }
        tracing::info!(target: "cache", freed, remaining = total, "image cache trimmed");
        Ok(freed)
    }

    pub fn clear(&self) -> Result<()> {
        std::fs::remove_dir_all(&self.dir).map_err(|e| img_err(&e))?;
        std::fs::create_dir_all(&self.dir).map_err(|e| img_err(&e))
    }

    pub fn size_bytes(&self) -> u64 {
        let mut files = Vec::new();
        let _ = collect(&self.dir, &mut files);
        files.iter().map(|f| f.1).sum()
    }
}

fn collect(dir: &Path, out: &mut Vec<(PathBuf, u64, SystemTime)>) -> std::io::Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let meta = entry.metadata()?;
        if meta.is_dir() {
            collect(&entry.path(), out)?;
        } else {
            out.push((entry.path(), meta.len(), meta.modified().unwrap_or(SystemTime::UNIX_EPOCH)));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use oneshot_core::media::ImageKind;
    use oneshot_core::{ItemRef, ServerId};

    use super::*;

    #[test]
    fn tmdb_and_avatar_keys_never_collide() {
        let url = url::Url::parse("https://image.tmdb.org/t/p/w342/abc.jpg").unwrap();
        let tmdb = tmdb_cache_key(&url);
        assert_eq!(tmdb.len(), 64);
        assert_ne!(tmdb, avatar_cache_key(&url));
        assert_eq!(tmdb, tmdb_cache_key(&url), "stable");
    }

    #[test]
    fn keys_depend_on_size_and_do_not_leak_tags() {
        let img = ImageRef { item: ItemRef::new(ServerId::new(), "1"), kind: ImageKind::Poster, tag: "/secret?X-Plex-Token=abc".into(), blurhash: None };
        let a = cache_key(&img, ImageSize::Card);
        let b = cache_key(&img, ImageSize::Hero);
        assert_ne!(a, b);
        assert!(!a.contains("abc") && a.len() == 64);
    }

    #[test]
    fn concurrent_writers_of_one_key_never_mix_their_bytes() {
        let dir = tempfile::tempdir().unwrap();
        let cache = ImageCache::new(dir.path().to_path_buf(), u64::MAX).unwrap();
        let key = format!("{:0>64}", 7);
        let writers: Vec<_> = (0..8u8)
            .map(|i| {
                let cache = cache.clone();
                let key = key.clone();
                std::thread::spawn(move || cache.put(&key, &vec![i; 64 * 1024 + usize::from(i)]).unwrap())
            })
            .collect();
        for w in writers {
            w.join().unwrap();
        }
        let bytes = cache.get(&key).unwrap();
        let first = bytes[0];
        assert!(bytes.iter().all(|b| *b == first), "one writer's picture, whole");
        assert_eq!(bytes.len(), 64 * 1024 + usize::from(first));
        let leftovers = std::fs::read_dir(dir.path().join(&key[..2])).unwrap().count();
        assert_eq!(leftovers, 1, "no temp file left behind");
    }

    #[test]
    fn lru_eviction_keeps_budget() {
        let dir = tempfile::tempdir().unwrap();
        let cache = ImageCache::new(dir.path().to_path_buf(), 2_000).unwrap();
        for i in 0..5 {
            let key = format!("{i:0>64}");
            cache.put(&key, &vec![0u8; 1_000]).unwrap();
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        cache.enforce_limit().unwrap();
        assert!(cache.size_bytes() <= 2_000);
        assert!(cache.get(&format!("{:0>64}", 4)).is_some(), "most recent kept");
        assert!(cache.get(&format!("{:0>64}", 0)).is_none(), "oldest evicted");
    }
}
