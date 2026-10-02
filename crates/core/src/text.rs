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

#[cfg(test)]
mod tests {
    use super::*;

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
