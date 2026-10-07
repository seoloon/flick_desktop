//! Jellyfin DTOs → Flick domain model.

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

use crate::dto::{BaseItemDto, MediaSegmentDto, MediaSourceInfo, MediaStream, UserItemData};

pub const TICKS_PER_MS: i64 = 10_000;

pub fn ticks_to_ms(ticks: i64) -> u64 {
    (ticks.max(0) / TICKS_PER_MS) as u64
}

pub fn kind(t: &str) -> ItemKind {
    match t {
        "Movie" => ItemKind::Movie,
        "Series" => ItemKind::Series,
        "Season" => ItemKind::Season,
        "Episode" => ItemKind::Episode,
        "BoxSet" => ItemKind::Collection,
        "Playlist" => ItemKind::Playlist,
        "Person" => ItemKind::Person,
        "Folder" | "CollectionFolder" | "UserView" => ItemKind::Folder,
        "MusicVideo" => ItemKind::MusicVideo,
        "Video" => ItemKind::Video,
        "TvChannel" => ItemKind::TvChannel,
        _ => ItemKind::Other,
    }
}

pub fn item_type_name(k: ItemKind) -> Option<&'static str> {
    Some(match k {
        ItemKind::Movie => "Movie",
        ItemKind::Series => "Series",
        ItemKind::Season => "Season",
        ItemKind::Episode => "Episode",
        ItemKind::Collection => "BoxSet",
        ItemKind::Playlist => "Playlist",
        ItemKind::Person => "Person",
        ItemKind::MusicVideo => "MusicVideo",
        ItemKind::Video => "Video",
        ItemKind::TvChannel => "TvChannel",
        ItemKind::Folder => "Folder",
        ItemKind::Other => return None,
    })
}

fn parse_datetime(s: &Option<String>) -> Option<DateTime<Utc>> {
    s.as_deref().and_then(|s| DateTime::parse_from_rfc3339(s).ok()).map(|d| d.with_timezone(&Utc))
}

pub(crate) fn parse_date(s: &Option<String>) -> Option<NaiveDate> {
    s.as_deref().and_then(|s| s.get(..10)).and_then(|d| NaiveDate::parse_from_str(d, "%Y-%m-%d").ok())
}

fn image(server: ServerId, owner: &str, kind: ImageKind, jf_type: &str, tag: &str, dto: &BaseItemDto) -> ImageRef {
    let blurhash = dto.image_blur_hashes.get(jf_type).and_then(|m| m.get(tag)).cloned();
    // The tag keeps Jellyfin's image type: an episode's "Primary" still is
    // exposed as our Thumb, so the kind alone cannot rebuild the URL.
    ImageRef { item: ItemRef::new(server, owner), kind, tag: format!("{jf_type}/{tag}"), blurhash }
}

pub fn images(server: ServerId, dto: &BaseItemDto) -> ImageSet {
    let own = |jf: &str, kind: ImageKind| dto.image_tags.get(jf).map(|t| image(server, &dto.id, kind, jf, t, dto));
    let mut set = ImageSet {
        poster: own("Primary", ImageKind::Poster),
        backdrop: dto.backdrop_image_tags.first().map(|t| image(server, &dto.id, ImageKind::Backdrop, "Backdrop", t, dto)),
        thumb: own("Thumb", ImageKind::Thumb),
        logo: own("Logo", ImageKind::Logo),
        banner: own("Banner", ImageKind::Banner),
    };
    // Episodes and seasons inherit artwork from their series.
    if set.backdrop.is_none()
        && let (Some(id), Some(tag)) = (&dto.parent_backdrop_item_id, dto.parent_backdrop_image_tags.first())
    {
        set.backdrop = Some(image(server, id, ImageKind::Backdrop, "Backdrop", tag, dto));
    }
    if set.logo.is_none()
        && let (Some(id), Some(tag)) = (&dto.parent_logo_item_id, &dto.parent_logo_image_tag)
    {
        set.logo = Some(image(server, id, ImageKind::Logo, "Logo", tag, dto));
    }
    if set.thumb.is_none()
        && let (Some(id), Some(tag)) = (&dto.parent_thumb_item_id, &dto.parent_thumb_image_tag)
    {
        set.thumb = Some(image(server, id, ImageKind::Thumb, "Thumb", tag, dto));
    }
    if kind(&dto.r#type) == ItemKind::Episode {
        // The episode "Primary" is a 16:9 still: use it as thumb, and the
        // series poster as poster so grids stay uniform.
        if set.thumb.is_none() {
            set.thumb = set.poster.take().map(|mut p| {
                p.kind = ImageKind::Thumb;
                p
            });
        }
        if let (Some(id), Some(tag)) = (&dto.series_id, &dto.series_primary_image_tag) {
            set.poster = Some(image(server, id, ImageKind::Poster, "Primary", tag, dto));
        }
    }
    set
}

fn user_state(u: Option<&UserItemData>) -> UserState {
    let Some(u) = u else { return UserState::default() };
    UserState {
        played: u.played,
        play_count: u.play_count,
        position_ms: ticks_to_ms(u.playback_position_ticks),
        favorite: u.is_favorite,
        last_played: parse_datetime(&u.last_played_date),
        unplayed_count: u.unplayed_item_count,
    }
}

fn role(t: Option<&str>) -> PersonRole {
    match t {
        Some("Actor") => PersonRole::Actor,
        Some("Director") => PersonRole::Director,
        Some("Writer") => PersonRole::Writer,
        Some("Producer") => PersonRole::Producer,
        Some("Composer") => PersonRole::Composer,
        Some("GuestStar") => PersonRole::GuestStar,
        _ => PersonRole::Other,
    }
}

pub fn item(server: ServerId, dto: &BaseItemDto) -> MediaItem {
    let k = kind(&dto.r#type);
    let mut item = MediaItem::new(ItemRef::new(server, &dto.id), k, dto.name.clone().unwrap_or_default());
    item.sort_title = dto.sort_name.clone();
    item.original_title = dto.original_title.clone().filter(|o| Some(o) != dto.name.as_ref());
    item.original_language = oneshot_core::text::guess_original_language(dto.original_title.as_deref(), &dto.production_locations);
    item.tagline = dto.taglines.first().cloned();
    item.overview = dto.overview.clone();
    item.year = dto.production_year;
    item.premiere_date = parse_date(&dto.premiere_date);
    item.runtime_ms = dto.run_time_ticks.map(ticks_to_ms);
    item.official_rating = dto.official_rating.clone();
    item.community_rating = dto.community_rating;
    item.critic_rating = dto.critic_rating;
    item.genres = dto.genres.clone();
    item.studios = dto.studios.iter().map(|s| s.name.clone()).collect();
    item.credits = dto
        .people
        .iter()
        .map(|p| Credit {
            person: ItemRef::new(server, &p.id),
            name: p.name.clone(),
            role: role(p.r#type.as_deref()),
            character: p.role.clone().filter(|r| !r.is_empty()),
            image: p.primary_image_tag.as_ref().map(|t| ImageRef {
                item: ItemRef::new(server, &p.id),
                kind: ImageKind::Poster,
                tag: format!("Primary/{t}"),
                blurhash: None,
            }),
        })
        .collect();
    item.images = images(server, dto);
    item.user = user_state(dto.user_data.as_ref());
    item.external_ids = external_ids(dto);
    item.child_count = dto.child_count.or(dto.recursive_item_count);
    item.added_at = parse_datetime(&dto.date_created);
    if matches!(k, ItemKind::Episode | ItemKind::Season) {
        item.episode = Some(EpisodeInfo {
            series: dto.series_id.as_ref().map(|id| ItemRef::new(server, id)),
            series_title: dto.series_name.clone(),
            season: dto.season_id.as_ref().map(|id| ItemRef::new(server, id)),
            season_number: if k == ItemKind::Season { dto.index_number } else { dto.parent_index_number },
            episode_number: (k == ItemKind::Episode).then_some(dto.index_number).flatten(),
            episode_number_end: dto.index_number_end,
        });
    }
    item.sources = dto.media_sources.iter().map(source).collect();
    item
}

pub fn library(server: ServerId, dto: &BaseItemDto) -> Library {
    let kind = match dto.collection_type.as_deref() {
        Some("movies") => LibraryKind::Movies,
        Some("tvshows") => LibraryKind::Shows,
        Some("music") => LibraryKind::Music,
        Some("photos") => LibraryKind::Photos,
        Some("musicvideos") => LibraryKind::MusicVideos,
        Some("homevideos") => LibraryKind::HomeVideos,
        Some("livetv") => LibraryKind::LiveTv,
        None | Some("mixed") => LibraryKind::Mixed,
        _ => LibraryKind::Other,
    };
    Library {
        id: ItemRef::new(server, &dto.id),
        name: dto.name.clone().unwrap_or_default(),
        kind,
        // `ChildCount` is the top-level folders, not the titles: the
        // provider counts those separately.
        item_count: None,
        image: images(server, dto).poster,
    }
}

fn range(s: &MediaStream) -> DynamicRange {
    let base = s.video_range_type.as_deref().map_or(DynamicRange::Unknown, DynamicRange::from_jellyfin_range_type);
    match base {
        DynamicRange::DolbyVision { compat, enhancement_layer, .. } => {
            let compat = match s.dv_bl_signal_compatibility_id {
                Some(0) => DolbyVisionCompat::None,
                Some(1 | 6) => DolbyVisionCompat::Hdr10,
                Some(2) => DolbyVisionCompat::Sdr,
                Some(4) => DolbyVisionCompat::Hlg,
                _ => compat,
            };
            DynamicRange::DolbyVision {
                profile: s.dv_profile,
                compat,
                enhancement_layer: enhancement_layer || s.el_present_flag == Some(1),
            }
        }
        other => other,
    }
}

fn spatial(s: &MediaStream) -> Option<SpatialAudio> {
    let hay = format!("{} {}", s.profile.as_deref().unwrap_or(""), s.display_title.as_deref().unwrap_or("")).to_lowercase();
    if hay.contains("atmos") {
        Some(SpatialAudio::DolbyAtmos)
    } else if hay.contains("dts:x") || hay.contains("dts-x") {
        Some(SpatialAudio::DtsX)
    } else {
        None
    }
}

pub fn source(src: &MediaSourceInfo) -> MediaSource {
    let mut out = MediaSource {
        id: src.id.clone(),
        name: src.name.clone(),
        container: src.container.clone(),
        size_bytes: src.size.map(|s| s.max(0) as u64),
        bitrate: src.bitrate.map(|b| b.max(0) as u64),
        duration_ms: src.run_time_ticks.map(ticks_to_ms),
        video: Vec::new(),
        audio: Vec::new(),
        subtitles: Vec::new(),
    };
    for s in &src.media_streams {
        let codec = s.codec.as_deref().unwrap_or_default();
        match s.r#type.as_str() {
            "Video" => out.video.push(VideoStream {
                index: s.index,
                codec: VideoCodec::parse(codec),
                profile: s.profile.clone(),
                level: s.level,
                width: s.width.unwrap_or(0),
                height: s.height.unwrap_or(0),
                bit_depth: s.bit_depth,
                frame_rate: s.real_frame_rate.or(s.average_frame_rate),
                bitrate: s.bit_rate.map(|b| b.max(0) as u64),
                range: range(s),
                interlaced: s.is_interlaced,
                title: s.title.clone(),
                is_default: s.is_default,
            }),
            "Audio" => out.audio.push(AudioStream {
                index: s.index,
                codec: AudioCodec::parse(codec, s.profile.as_deref()),
                profile: s.profile.clone(),
                channels: s.channels.unwrap_or(2),
                channel_layout: s.channel_layout.clone(),
                sample_rate: s.sample_rate,
                bitrate: s.bit_rate.map(|b| b.max(0) as u64),
                spatial: spatial(s),
                language: s.language.clone(),
                title: s.title.clone().or_else(|| s.display_title.clone()),
                is_default: s.is_default,
                is_commentary: s.title.as_deref().is_some_and(|t| t.to_lowercase().contains("comment")),
            }),
            "Subtitle" => out.subtitles.push(SubtitleStream {
                index: s.index,
                format: SubtitleFormat::parse(codec),
                language: s.language.clone(),
                title: s.title.clone().or_else(|| s.display_title.clone()),
                forced: s.is_forced,
                hearing_impaired: s.is_hearing_impaired,
                is_default: s.is_default,
                external: s.is_external,
                delivery_path: s.delivery_url.clone(),
            }),
            _ => {}
        }
    }
    out
}

pub fn marker(seg: &MediaSegmentDto) -> Option<Marker> {
    let kind = match seg.r#type.as_str() {
        "Intro" => MarkerKind::Intro,
        "Outro" => MarkerKind::Credits,
        "Recap" => MarkerKind::Recap,
        "Preview" => MarkerKind::Preview,
        "Commercial" => MarkerKind::Commercial,
        _ => return None,
    };
    Some(Marker { kind, start_ms: ticks_to_ms(seg.start_ticks), end_ms: ticks_to_ms(seg.end_ticks) })
}

pub(crate) fn external_ids(dto: &BaseItemDto) -> ExternalIds {
    ExternalIds {
        imdb: dto.provider_ids.get("Imdb").cloned(),
        tmdb: dto.provider_ids.get("Tmdb").cloned(),
        tvdb: dto.provider_ids.get("Tvdb").cloned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const EPISODE: &str = include_str!("../tests/fixtures/episode.json");

    #[test]
    fn maps_episode_with_sources_and_inherited_art() {
        let dto: BaseItemDto = serde_json::from_str(EPISODE).unwrap();
        let server = ServerId::new();
        let item = item(server, &dto);
        assert_eq!(item.kind, ItemKind::Episode);
        assert_eq!(item.runtime_ms, Some(3_000_000));
        assert_eq!(item.user.position_ms, 1_200_000);
        let ep = item.episode.as_ref().unwrap();
        assert_eq!((ep.season_number, ep.episode_number), (Some(2), Some(5)));
        // poster comes from the series, still becomes the thumb
        assert_eq!(item.images.poster.as_ref().unwrap().item.key, "series1");
        assert_eq!(item.images.thumb.as_ref().unwrap().kind, ImageKind::Thumb);
        assert_eq!(item.images.backdrop.as_ref().unwrap().item.key, "series1");

        let src = &item.sources[0];
        let v = src.primary_video().unwrap();
        assert_eq!(v.codec, VideoCodec::Hevc);
        assert_eq!(
            v.range,
            DynamicRange::DolbyVision { profile: Some(8), compat: DolbyVisionCompat::Hdr10, enhancement_layer: false }
        );
        assert_eq!(src.audio[0].codec, AudioCodec::TrueHd);
        assert_eq!(src.audio[0].spatial, Some(SpatialAudio::DolbyAtmos));
        assert_eq!(src.audio[1].codec, AudioCodec::DtsHd);
        assert_eq!(src.subtitles[0].format, SubtitleFormat::Pgs);
        assert!(src.subtitles[1].forced);
    }

    #[test]
    fn segments_map_to_markers() {
        let s = MediaSegmentDto { r#type: "Intro".into(), start_ticks: 10_000_000, end_ticks: 900_000_000 };
        let m = marker(&s).unwrap();
        assert_eq!((m.kind, m.start_ms, m.end_ms), (MarkerKind::Intro, 1000, 90_000));
    }
}
