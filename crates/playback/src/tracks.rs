//! Default track selection.

use oneshot_core::settings::{SubtitleMode, SubtitleSettings};
use oneshot_core::stream::{AudioStream, MediaSource, SubtitleStream};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase", tag = "type", content = "index")]
pub enum TrackRequest {
    /// Let the engine choose from preferences and file defaults.
    #[default]
    Auto,
    Off,
    Index(u32),
}

/// Normalises ISO 639-1/639-2 codes so "fr", "fre" and "fra" compare equal.
pub fn same_language(a: &str, b: &str) -> bool {
    fn norm(s: &str) -> String {
        let s = s.to_ascii_lowercase();
        match s.as_str() {
            "fre" | "fra" => "fr".into(),
            "eng" => "en".into(),
            "ger" | "deu" => "de".into(),
            "spa" => "es".into(),
            "ita" => "it".into(),
            "jpn" => "ja".into(),
            "por" => "pt".into(),
            "dut" | "nld" => "nl".into(),
            "chi" | "zho" => "zh".into(),
            "kor" => "ko".into(),
            "rus" => "ru".into(),
            _ => s.split(['-', '_']).next().unwrap_or_default().to_owned(),
        }
    }
    norm(a) == norm(b)
}

pub fn select_audio<'a>(source: &'a MediaSource, request: TrackRequest, languages: &[String]) -> Option<&'a AudioStream> {
    match request {
        TrackRequest::Off => None,
        TrackRequest::Index(i) => source.audio.iter().find(|a| a.index == i).or_else(|| source.default_audio()),
        TrackRequest::Auto => languages
            .iter()
            .find_map(|lang| {
                // Prefer the richest non-commentary track in the preferred language.
                source
                    .audio
                    .iter()
                    .filter(|a| !a.is_commentary && a.language.as_deref().is_some_and(|l| same_language(l, lang)))
                    .max_by_key(|a| (a.is_default, a.channels))
            })
            .or_else(|| source.default_audio()),
    }
}

pub fn select_subtitle<'a>(
    source: &'a MediaSource,
    request: TrackRequest,
    prefs: &SubtitleSettings,
    audio: Option<&AudioStream>,
) -> Option<&'a SubtitleStream> {
    match request {
        TrackRequest::Off => None,
        TrackRequest::Index(i) => source.subtitles.iter().find(|s| s.index == i),
        TrackRequest::Auto => {
            let audio_lang = audio.and_then(|a| a.language.as_deref());
            let in_lang = |s: &&SubtitleStream, lang: &str| s.language.as_deref().is_some_and(|l| same_language(l, lang));
            let forced_for_audio = || {
                source.subtitles.iter().find(|s| s.forced && audio_lang.is_none_or(|al| in_lang(s, al)))
            };
            let full_in_prefs = || {
                prefs.languages.iter().find_map(|lang| {
                    source.subtitles.iter().filter(|s| !s.forced).find(|s| in_lang(s, lang))
                })
            };
            match prefs.mode {
                SubtitleMode::Off => None,
                SubtitleMode::ForcedOnly => forced_for_audio(),
                SubtitleMode::Always => full_in_prefs().or_else(forced_for_audio),
                SubtitleMode::Smart => {
                    let audio_is_preferred = audio_lang
                        .is_some_and(|al| prefs.languages.is_empty() || prefs.languages.iter().any(|l| same_language(l, al)));
                    if audio_is_preferred { forced_for_audio() } else { full_in_prefs().or_else(forced_for_audio) }
                }
            }
        }
    }
}
