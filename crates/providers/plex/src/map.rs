//! Plex payloads → Flick domain model.

use chrono::{DateTime, NaiveDate, Utc};
use oneshot_core::ServerId;
use oneshot_core::ids::ItemRef;
use oneshot_core::media::{
    Credit, EpisodeInfo, ExternalIds, ImageKind, ImageRef, ImageSet, ItemKind, Marker, MarkerKind, MediaItem,
    PersonRole, UserState,
};
use oneshot_core::server::{Library, LibraryKind};
use oneshot_core::stream::{
    AudioCodec, AudioStream, DolbyVisionCompat, DynamicRange, MediaSource, SpatialAudio, SubtitleFormat,
    SubtitleStream, VideoCodec, VideoStream,
};

use crate::dto::{Directory, Media, Metadata, Stream, Tag};

pub fn kind(t: &str) -> ItemKind {
    match t {
        "movie" => ItemKind::Movie,
        "show" => ItemKind::Series,
        "season" => ItemKind::Season,
        "episode" => ItemKind::Episode,
        "collection" => ItemKind::Collection,
        "playlist" => ItemKind::Playlist,
        "clip" => ItemKind::Video,
        _ => ItemKind::Other,
    }
}

/// Plex numeric `type` used by `/library/sections/{id}/all?type=`.
pub fn type_number(k: ItemKind) -> Option<u8> {
    Some(match k {
        ItemKind::Movie => 1,
        ItemKind::Series => 2,
        ItemKind::Season => 3,
        ItemKind::Episode => 4,
        ItemKind::Collection => 18,
        _ => return None,
    })
}

fn img(server: ServerId, rating_key: &str, kind: ImageKind, path: &Option<String>) -> Option<ImageRef> {
    // Plex image paths are server-relative and already cache-busted
    // (`/library/metadata/1/thumb/1712345678`): the path *is* the tag.
    path.as_ref()
        .filter(|p| !p.is_empty())
        .map(|p| ImageRef { item: ItemRef::new(server, rating_key), kind, tag: p.clone(), blurhash: None })
}

pub fn images(server: ServerId, m: &Metadata) -> ImageSet {
    let k = kind(&m.r#type);
    let key = &m.rating_key;
    match k {
        ItemKind::Episode => ImageSet {
            poster: img(server, key, ImageKind::Poster, &m.grandparent_thumb.clone().or(m.parent_thumb.clone())),
            backdrop: img(server, key, ImageKind::Backdrop, &m.grandparent_art.clone().or(m.art.clone())),
            thumb: img(server, key, ImageKind::Thumb, &m.thumb),
            logo: None,
            banner: None,
        },
        ItemKind::Season => ImageSet {
            poster: img(server, key, ImageKind::Poster, &m.thumb.clone().or(m.parent_thumb.clone())),
            backdrop: img(server, key, ImageKind::Backdrop, &m.art),
            ..ImageSet::default()
        },
        _ => ImageSet {
            poster: img(server, key, ImageKind::Poster, &m.thumb),
            backdrop: img(server, key, ImageKind::Backdrop, &m.art),
            ..ImageSet::default()
        },
    }
}

fn from_unix(ts: Option<i64>) -> Option<DateTime<Utc>> {
    ts.and_then(|t| DateTime::from_timestamp(t, 0))
}

fn external_ids(m: &Metadata) -> ExternalIds {
    let pick = |scheme: &str| {
        m.guids.iter().find_map(|g| g.id.strip_prefix(scheme).map(str::to_owned))
    };
    ExternalIds { imdb: pick("imdb://"), tmdb: pick("tmdb://"), tvdb: pick("tvdb://") }
}

fn credits(server: ServerId, tags: &[Tag], role: PersonRole) -> impl Iterator<Item = Credit> + '_ {
    tags.iter().map(move |t| Credit {
        person: ItemRef::new(server, t.id.map_or_else(|| format!("tag:{}", t.tag), |id| id.to_string())),
        name: t.tag.clone(),
        role,
        character: t.role.clone(),
        image: None,
    })
}

pub fn item(server: ServerId, m: &Metadata) -> MediaItem {
    let k = kind(&m.r#type);
    let mut item = MediaItem::new(ItemRef::new(server, &m.rating_key), k, m.title.clone());
    item.sort_title = m.title_sort.clone();
    item.original_title = m.original_title.clone();
    let countries: Vec<String> = m.countries.iter().map(|c| c.tag.clone()).collect();
    item.original_language = oneshot_core::text::guess_original_language(m.original_title.as_deref(), &countries);
    item.tagline = m.tagline.clone();
    item.overview = m.summary.clone().filter(|s| !s.is_empty());
    item.year = m.year;
    item.premiere_date = m.originally_available_at.as_deref().and_then(|d| NaiveDate::parse_from_str(d, "%Y-%m-%d").ok());
    item.runtime_ms = m.duration;
    item.official_rating = m.content_rating.clone();
    item.community_rating = m.audience_rating;
    item.critic_rating = m.rating;
    item.genres = m.genres.iter().map(|g| g.tag.clone()).collect();
    item.studios = m.studio.iter().cloned().collect();
    item.credits = credits(server, &m.roles, PersonRole::Actor)
        .chain(credits(server, &m.directors, PersonRole::Director))
        .chain(credits(server, &m.writers, PersonRole::Writer))
        .collect();
    item.images = images(server, m);
    let unplayed = m.leaf_count.map(|l| l.saturating_sub(m.viewed_leaf_count.unwrap_or(0)));
    item.user = UserState {
        played: match k {
            ItemKind::Series | ItemKind::Season => unplayed == Some(0),
            _ => m.view_count.unwrap_or(0) > 0,
        },
        play_count: m.view_count.unwrap_or(0),
        position_ms: m.view_offset.unwrap_or(0),
        // Plex has no favourites on library items; the provider marks
        // Watchlist titles (see `watchlist`).
        favorite: false,
        last_played: from_unix(m.last_viewed_at),
        unplayed_count: unplayed,
    };
    item.external_ids = external_ids(m);
    item.child_count = m.child_count.or(m.leaf_count);
    item.added_at = from_unix(m.added_at);
    if matches!(k, ItemKind::Episode | ItemKind::Season) {
        let (series, season) = match k {
            ItemKind::Episode => (m.grandparent_rating_key.clone(), m.parent_rating_key.clone()),
            _ => (m.parent_rating_key.clone(), None),
        };
        item.episode = Some(EpisodeInfo {
            series: series.map(|s| ItemRef::new(server, s)),
            series_title: if k == ItemKind::Episode { m.grandparent_title.clone() } else { m.parent_title.clone() },
            season: season.map(|s| ItemRef::new(server, s)),
            season_number: if k == ItemKind::Season { m.index } else { m.parent_index },
            episode_number: (k == ItemKind::Episode).then_some(m.index).flatten(),
            episode_number_end: None,
        });
    }
    item.sources = m.media.iter().map(source).collect();
    item
}

pub fn library(server: ServerId, d: &Directory) -> Library {
    let kind = match d.r#type.as_deref() {
        Some("movie") => LibraryKind::Movies,
        Some("show") => LibraryKind::Shows,
        Some("artist") => LibraryKind::Music,
        Some("photo") => LibraryKind::Photos,
        _ => LibraryKind::Other,
    };
    Library {
        id: ItemRef::new(server, format!("section:{}", d.key)),
        name: d.title.clone(),
        kind,
        item_count: None,
        image: d.thumb.as_ref().or(d.composite.as_ref()).map(|p| ImageRef {
            item: ItemRef::new(server, format!("section:{}", d.key)),
            kind: ImageKind::Poster,
            tag: p.clone(),
            blurhash: None,
        }),
    }
}

fn range(s: &Stream) -> DynamicRange {
    if s.dovi_present == Some(true) || s.dovi_profile.is_some() {
        return DynamicRange::DolbyVision {
            profile: s.dovi_profile,
            compat: match s.dovi_bl_compat_id {
                Some(0) => DolbyVisionCompat::None,
                Some(2) => DolbyVisionCompat::Sdr,
                Some(4) => DolbyVisionCompat::Hlg,
                _ => DolbyVisionCompat::Hdr10,
            },
            enhancement_layer: s.dovi_el_present == Some(true),
        };
    }
    let title = s.extended_display_title.as_deref().or(s.display_title.as_deref()).unwrap_or_default();
    match s.color_trc.as_deref() {
        Some("smpte2084") if title.contains("HDR10+") => DynamicRange::Hdr10Plus,
        Some("smpte2084") => DynamicRange::Hdr10,
        Some("arib-std-b67") => DynamicRange::Hlg,
        Some(_) => DynamicRange::Sdr,
        // Untagged 8-bit video is SDR in practice; untagged 10-bit stays
        // unknown and is resolved at runtime from mpv's video-params.
        None if s.bit_depth.unwrap_or(8) <= 8 => DynamicRange::Sdr,
        None => DynamicRange::Unknown,
    }
}

fn spatial(s: &Stream) -> Option<SpatialAudio> {
    let t = format!(
        "{} {} {}",
        s.profile.as_deref().unwrap_or(""),
        s.display_title.as_deref().unwrap_or(""),
        s.extended_display_title.as_deref().unwrap_or("")
    )
    .to_lowercase();
    if t.contains("atmos") {
        Some(SpatialAudio::DolbyAtmos)
    } else if t.contains("dts:x") || t.contains("dts-x") {
        Some(SpatialAudio::DtsX)
    } else {
        None
    }
}

/// One `Media` = one version. Multi-part media (CD1/CD2) exposes its first
/// part; stacked files are rare in modern libraries and flagged in logs.
pub fn source(media: &Media) -> MediaSource {
    let part = media.parts.first();
    if media.parts.len() > 1 {
        tracing::warn!(target: "provider", media = media.id, parts = media.parts.len(), "multi-part media: only part 1 is playable");
    }
    let mut out = MediaSource {
        id: media.id.to_string(),
        name: media.title.clone(),
        container: part.and_then(|p| p.container.clone()).or(media.container.clone()),
        size_bytes: part.and_then(|p| p.size),
        bitrate: media.bitrate.map(|kbps| kbps * 1000),
        duration_ms: media.duration.or(part.and_then(|p| p.duration)),
        video: Vec::new(),
        audio: Vec::new(),
        subtitles: Vec::new(),
    };
    let Some(part) = part else { return out };
    for (pos, s) in part.streams.iter().enumerate() {
        // Plex stream ids are global; mpv needs per-type ordering, which the
        // player derives from `index`. Fall back to position when absent.
        let index = s.index.unwrap_or(pos as u32);
        let codec = s.codec.as_deref().unwrap_or_default();
        let lang = s.language_tag.clone().or(s.language_code.clone());
        match s.stream_type {
            1 => out.video.push(VideoStream {
                index,
                codec: VideoCodec::parse(codec),
                profile: s.profile.clone(),
                level: s.level,
                width: s.width.unwrap_or(0),
                height: s.height.unwrap_or(0),
                bit_depth: s.bit_depth,
                frame_rate: s.frame_rate,
                bitrate: s.bitrate.map(|k| k * 1000),
                range: range(s),
                interlaced: s.scan_type.as_deref() == Some("interlaced"),
                title: s.title.clone(),
                is_default: s.default.unwrap_or(false) || s.selected.unwrap_or(false),
            }),
            2 => out.audio.push(AudioStream {
                index,
                codec: AudioCodec::parse(codec, s.profile.as_deref()),
                profile: s.profile.clone(),
                channels: s.channels.unwrap_or(2),
                channel_layout: s.audio_channel_layout.clone(),
                sample_rate: s.sampling_rate,
                bitrate: s.bitrate.map(|k| k * 1000),
                spatial: spatial(s),
                language: lang,
                title: s.title.clone().or(s.display_title.clone()),
                is_default: s.selected.unwrap_or(false) || s.default.unwrap_or(false),
                is_commentary: s.title.as_deref().is_some_and(|t| t.to_lowercase().contains("comment")),
            }),
            3 => out.subtitles.push(SubtitleStream {
                index: s.index.unwrap_or(10_000 + s.id as u32),
                format: SubtitleFormat::parse(codec),
                language: lang,
                title: s.title.clone().or(s.display_title.clone()),
                forced: s.forced.unwrap_or(false),
                hearing_impaired: s.hearing_impaired.unwrap_or(false),
                is_default: s.selected.unwrap_or(false),
                external: s.key.is_some(),
                delivery_path: s.key.clone(),
            }),
            _ => {}
        }
    }
    out
}

pub fn markers(m: &Metadata) -> Vec<Marker> {
    m.markers
        .iter()
        .filter_map(|mk| {
            let kind = match mk.r#type.as_str() {
                "intro" => MarkerKind::Intro,
                "credits" => MarkerKind::Credits,
                "commercial" => MarkerKind::Commercial,
                _ => return None,
            };
            Some(Marker { kind, start_ms: mk.start_time_offset, end_ms: mk.end_time_offset })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dto::Envelope;

    const EPISODE: &str = include_str!("../tests/fixtures/episode.json");

    #[test]
    fn maps_episode_metadata_streams_and_markers() {
        let env: Envelope<crate::dto::Container> = serde_json::from_str(EPISODE).unwrap();
        let m = &env.container.metadata[0];
        let server = ServerId::new();
        let item = item(server, m);
        assert_eq!(item.kind, ItemKind::Episode);
        assert_eq!(item.user.position_ms, 600_000);
        assert_eq!(item.external_ids.tvdb.as_deref(), Some("555"));
        let ep = item.episode.as_ref().unwrap();
        assert_eq!((ep.season_number, ep.episode_number), (Some(1), Some(3)));
        assert_eq!(ep.series.as_ref().unwrap().key, "100");
        assert!(item.images.poster.as_ref().unwrap().tag.starts_with("/library/metadata/100/thumb"));

        let src = &item.sources[0];
        assert_eq!(src.bitrate, Some(40_000_000));
        let v = src.primary_video().unwrap();
        assert_eq!(v.range, DynamicRange::DolbyVision { profile: Some(8), compat: DolbyVisionCompat::Hdr10, enhancement_layer: false });
        assert_eq!(src.audio[0].codec, AudioCodec::Eac3);
        assert_eq!(src.audio[0].spatial, Some(SpatialAudio::DolbyAtmos));
        assert_eq!(src.audio[1].codec, AudioCodec::DtsHd);
        assert!(src.subtitles.iter().any(|s| s.external && s.delivery_path.as_deref() == Some("/library/streams/77")));

        let mk = markers(m);
        assert_eq!(mk.len(), 2);
        assert_eq!(mk[0].kind, MarkerKind::Intro);
    }
}
