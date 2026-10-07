//! Text rules shared across crates.

use unicode_normalization::UnicodeNormalization;
use unicode_normalization::char::is_combining_mark;

/// Matching key for a person's name: case, accents and spacing ignored.
pub fn normalize_name(name: &str) -> String {
    let folded: String = name.nfkd().filter(|c| !is_combining_mark(*c)).collect::<String>().to_lowercase();
    folded.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The server's own spelling of `wanted` among its `genres` (case, accents
/// and spacing ignored), so "science fiction" finds "Science Fiction".
pub fn find_genre<'a>(genres: &'a [String], wanted: &str) -> Option<&'a String> {
    let key = normalize_name(wanted);
    genres.iter().find(|g| normalize_name(g) == key)
}

/// The language a title was made in, as far as the server's own metadata
/// tells: the script of its original title (kana → Japanese, hangul →
/// Korean, …), else the language of its first production country. `None`
/// when that does not settle it (a Latin title from a multilingual country).
pub fn guess_original_language(original_title: Option<&str>, countries: &[String]) -> Option<String> {
    let first_country = countries.first().map(|c| c.trim().to_lowercase());
    let country = |names: &[&str]| first_country.as_deref().is_some_and(|c| names.contains(&c));
    if let Some(title) = original_title {
        let (mut han, mut other) = (false, None);
        for c in title.chars() {
            match c as u32 {
                0x3040..=0x30FF | 0x31F0..=0x31FF => return Some("ja".into()),
                0xAC00..=0xD7AF | 0x1100..=0x11FF => return Some("ko".into()),
                0x3400..=0x4DBF | 0x4E00..=0x9FFF => han = true,
                0x0400..=0x04FF if other.is_none() => other = Some(if country(&["ukraine"]) { "uk" } else { "ru" }),
                0x0E00..=0x0E7F => other = other.or(Some("th")),
                0x0600..=0x06FF => other = other.or(Some("ar")),
                0x0590..=0x05FF => other = other.or(Some("he")),
                0x0370..=0x03FF => other = other.or(Some("el")),
                0x0900..=0x097F => other = other.or(Some("hi")),
                _ => {}
            }
        }
        if han {
            return Some(if country(&["japan", "jp"]) { "ja" } else { "zh" }.into());
        }
        if let Some(l) = other {
            return Some(l.into());
        }
    }
    let table: [(&[&str], &str); 22] = [
        (&["japan", "jp"], "ja"),
        (&["south korea", "korea", "kr"], "ko"),
        (&["china", "taiwan", "hong kong", "cn", "tw", "hk"], "zh"),
        (&["united states of america", "united states", "usa", "us", "united kingdom", "uk", "gb", "australia", "new zealand", "ireland"], "en"),
        (&["france", "fr"], "fr"),
        (&["germany", "de"], "de"),
        (&["spain", "mexico", "argentina", "es", "mx", "ar"], "es"),
        (&["italy", "it"], "it"),
        (&["russia", "ru"], "ru"),
        (&["brazil", "portugal", "br", "pt"], "pt"),
        (&["thailand", "th"], "th"),
        (&["sweden", "se"], "sv"),
        (&["denmark", "dk"], "da"),
        (&["norway", "no"], "no"),
        (&["finland", "fi"], "fi"),
        (&["netherlands", "nl"], "nl"),
        (&["poland", "pl"], "pl"),
        (&["turkey", "tr"], "tr"),
        (&["ukraine", "ua"], "uk"),
        (&["greece", "gr"], "el"),
        (&["israel", "il"], "he"),
        (&["india", "in"], "hi"),
    ];
    table.iter().find(|(names, _)| country(names)).map(|(_, l)| (*l).to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn original_language_comes_from_the_title_script_then_the_country() {
        let c = |s: &str| vec![s.to_owned()];
        assert_eq!(guess_original_language(Some("鬼滅の刃"), &c("Japan")).as_deref(), Some("ja"));
        assert_eq!(guess_original_language(Some("寄生虫"), &c("South Korea")).as_deref(), Some("zh"));
        assert_eq!(guess_original_language(Some("기생충"), &[]).as_deref(), Some("ko"));
        assert_eq!(guess_original_language(Some("Inception"), &c("United States of America")).as_deref(), Some("en"));
        assert_eq!(guess_original_language(None, &c("France")).as_deref(), Some("fr"));
        assert_eq!(guess_original_language(Some("Inception"), &c("Canada")), None);
        assert_eq!(guess_original_language(None, &[]), None);
    }

    #[test]
    fn genres_are_found_by_their_normalized_name() {
        let genres = vec!["Action".to_owned(), "Science Fiction".to_owned(), "Drame".to_owned()];
        assert_eq!(find_genre(&genres, " science  fiction ").map(String::as_str), Some("Science Fiction"));
        assert_eq!(find_genre(&genres, "DRAME").map(String::as_str), Some("Drame"));
        assert!(find_genre(&genres, "Horror").is_none());
    }

    #[test]
    fn names_match_across_case_accents_and_spaces() {
        assert_eq!(normalize_name("  Antoine "), "antoine");
        assert_eq!(normalize_name("Antoïne"), "antoine");
        assert_eq!(normalize_name("Élodie   Martin"), "elodie martin");
        assert_ne!(normalize_name("Léa"), normalize_name("Leo"));
    }
}
