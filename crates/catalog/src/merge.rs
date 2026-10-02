//! Cross-server merging rules.

use oneshot_core::ids::ItemRef;
use oneshot_core::media::{ItemKind, MediaItem};
use oneshot_core::query::{HomeRow, HomeRowKind, SortOrder};
use oneshot_core::text::normalize_name;

/// Collapses items that are the same title on different servers (matched by
/// IMDb/TMDb/TVDB id and kind). The first occurrence wins and records the
/// others in `alternates`; items without external ids are never merged.
pub fn dedupe(items: Vec<MediaItem>) -> Vec<MediaItem> {
    let mut out: Vec<MediaItem> = Vec::with_capacity(items.len());
    for item in items {
        let dup = out.iter_mut().find(|o| {
            o.kind == item.kind
                && o.id.server != item.id.server
                && !item.external_ids.is_empty()
                && o.external_ids.matches(&item.external_ids)
        });
        match dup {
            Some(existing) => {
                existing.alternates.push(item.id);
                // Resume from whichever server has the most recent progress.
                if item.user.last_played > existing.user.last_played {
                    existing.user = item.user;
                }
            }
            None => out.push(item),
        }
    }
    out
}

/// One list of genres from several servers: the same genre spelled differently
/// (case, accents, spacing) appears once, under the first spelling met.
pub fn merge_genres(per_server: Vec<Vec<String>>) -> Vec<String> {
    let mut keyed: Vec<(String, String)> = Vec::new();
    for name in per_server.into_iter().flatten() {
        let key = normalize_name(&name);
        if !key.is_empty() && !keyed.iter().any(|(k, _)| *k == key) {
            keyed.push((key, name));
        }
    }
    keyed.sort_by(|a, b| a.0.cmp(&b.0));
    keyed.into_iter().map(|(_, name)| name).collect()
}

/// Orders titles from several servers together, as each server ordered its own.
pub fn sort_by_title(items: &mut [MediaItem], order: SortOrder) {
    let key = |m: &MediaItem| normalize_name(m.sort_title.as_deref().unwrap_or(&m.title));
    items.sort_by_cached_key(key);
    if order == SortOrder::Descending {
        items.reverse();
    }
}

/// A title the person watched, from which to look for similar ones.
#[derive(Debug, Clone, PartialEq)]
pub struct Seed {
    pub id: ItemRef,
    pub title: String,
}

/// The `count` most recently watched titles (movies and series; an episode
/// counts for its series), newest first, each title once.
pub fn recent_seeds(watched: Vec<MediaItem>, count: usize) -> Vec<Seed> {
    let mut dated: Vec<_> = watched
        .into_iter()
        .filter_map(|m| {
            let at = m.user.last_played?;
            let seed = match (&m.episode, m.kind) {
                (Some(e), _) => Seed { id: e.series.clone()?, title: e.series_title.clone()? },
                (None, ItemKind::Movie | ItemKind::Series) => Seed { id: m.id, title: m.title },
                _ => return None,
            };
            Some((at, seed))
        })
        .collect();
    dated.sort_by_key(|(at, _)| std::cmp::Reverse(*at));
    let mut seeds: Vec<Seed> = Vec::new();
    for (_, seed) in dated {
        let key = normalize_name(&seed.title);
        if !seeds.iter().any(|s| s.id == seed.id || normalize_name(&s.title) == key) {
            seeds.push(seed);
        }
        if seeds.len() == count {
            break;
        }
    }
    seeds
}

/// Rows of "Because you watched …" from what each seed's server found similar.
/// Watched titles and the seed itself are dropped, a title shows in one row
/// only, and a row too thin to be worth showing is left out.
pub fn recommendation_rows(found: Vec<(Seed, Vec<MediaItem>)>, per_row: usize) -> Vec<HomeRow> {
    const MIN_ROW: usize = 3;
    let mut shown: Vec<MediaItem> = Vec::new();
    let mut rows = Vec::new();
    for (seed, similar) in found {
        let seed_key = normalize_name(&seed.title);
        let mut items: Vec<MediaItem> = Vec::new();
        for item in similar {
            let same = |o: &MediaItem| o.id == item.id || (o.kind == item.kind && normalize_name(&o.title) == normalize_name(&item.title));
            if item.user.played || item.id == seed.id || normalize_name(&item.title) == seed_key || shown.iter().any(same) || items.iter().any(same) {
                continue;
            }
            items.push(item);
            if items.len() == per_row {
                break;
            }
        }
        if items.len() >= MIN_ROW {
            shown.extend(items.iter().cloned());
            rows.push(HomeRow { kind: HomeRowKind::Recommended, title: format!("Because you watched {}", seed.title), items });
        }
    }
    rows
}

/// Merges per-server home rows.
///
/// * Continue Watching / Next Up: one row, ordered by last activity.
/// * Recently Added: kept per library; with several servers the title gets
///   the server name so users know where it comes from.
/// * Provider-specific rows are kept as-is.
pub fn merge_rows(per_server: Vec<(String, Vec<HomeRow>)>, multi: bool) -> Vec<HomeRow> {
    let mut continue_watching: Vec<MediaItem> = Vec::new();
    let mut next_up: Vec<MediaItem> = Vec::new();
    let mut others: Vec<HomeRow> = Vec::new();
    for (server, rows) in per_server {
        for mut row in rows {
            match row.kind {
                HomeRowKind::ContinueWatching => continue_watching.extend(row.items),
                HomeRowKind::NextUp => next_up.extend(row.items),
                _ => {
                    if multi {
                        row.title = format!("{} from {server}", row.title);
                    }
                    others.push(row);
                }
            }
        }
    }
    let by_recency = |a: &MediaItem, b: &MediaItem| b.user.last_played.cmp(&a.user.last_played);
    continue_watching.sort_by(by_recency);
    next_up.sort_by(by_recency);

    let mut rows = Vec::new();
    if !continue_watching.is_empty() {
        rows.push(HomeRow { kind: HomeRowKind::ContinueWatching, title: "Continue Watching".into(), items: dedupe(continue_watching) });
    }
    if !next_up.is_empty() {
        rows.push(HomeRow { kind: HomeRowKind::NextUp, title: "Next Up".into(), items: dedupe(next_up) });
    }
    rows.extend(others);
    rows
}

#[cfg(test)]
mod tests {
    use chrono::{TimeZone, Utc};
    use oneshot_core::media::{ExternalIds, ItemKind};
    use oneshot_core::{ItemRef, ServerId};

    use super::*;

    fn movie(server: ServerId, key: &str, imdb: Option<&str>, played_at: i64) -> MediaItem {
        let mut m = MediaItem::new(ItemRef::new(server, key), ItemKind::Movie, key);
        m.external_ids = ExternalIds { imdb: imdb.map(str::to_owned), ..Default::default() };
        m.user.last_played = Some(Utc.timestamp_opt(played_at, 0).unwrap());
        m
    }

    #[test]
    fn same_title_on_two_servers_is_merged_with_latest_progress() {
        let (a, b) = (ServerId::new(), ServerId::new());
        let mut newer = movie(b, "b1", Some("tt1"), 200);
        newer.user.position_ms = 99;
        let merged = dedupe(vec![movie(a, "a1", Some("tt1"), 100), newer, movie(b, "b2", None, 0)]);
        assert_eq!(merged.len(), 2);
        assert_eq!(merged[0].alternates, vec![ItemRef::new(b, "b1")]);
        assert_eq!(merged[0].user.position_ms, 99);
    }

    #[test]
    fn items_without_ids_or_on_same_server_are_not_merged() {
        let a = ServerId::new();
        let merged = dedupe(vec![movie(a, "1", Some("tt1"), 0), movie(a, "2", Some("tt1"), 0)]);
        assert_eq!(merged.len(), 2, "two versions on one server are distinct items");
    }

    fn episode_of(server: ServerId, series: &str, title: &str, played_at: i64) -> MediaItem {
        let mut m = MediaItem::new(ItemRef::new(server, format!("ep-{series}-{played_at}")), ItemKind::Episode, "Pilot");
        m.user.last_played = Some(Utc.timestamp_opt(played_at, 0).unwrap());
        m.episode = Some(oneshot_core::media::EpisodeInfo {
            series: Some(ItemRef::new(server, series)),
            series_title: Some(title.into()),
            ..Default::default()
        });
        m
    }

    #[test]
    fn genres_from_several_servers_are_merged_and_sorted() {
        let merged = merge_genres(vec![
            vec!["Science Fiction".into(), "Action".into(), "Drame".into()],
            vec!["action".into(), "Éveil".into(), "science  fiction".into()],
        ]);
        assert_eq!(merged, vec!["Action", "Drame", "Éveil", "Science Fiction"]);
    }

    #[test]
    fn seeds_are_the_latest_distinct_titles_and_episodes_count_for_their_series() {
        let s = ServerId::new();
        let watched = vec![
            movie(s, "Old Movie", None, 10),
            episode_of(s, "show1", "Some Show", 50),
            episode_of(s, "show1", "Some Show", 40),
            movie(s, "New Movie", None, 60),
            MediaItem::new(ItemRef::new(s, "never"), ItemKind::Movie, "Never Watched"),
        ];
        let seeds = recent_seeds(watched, 3);
        let titles: Vec<&str> = seeds.iter().map(|x| x.title.as_str()).collect();
        assert_eq!(titles, vec!["New Movie", "Some Show", "Old Movie"]);
        assert_eq!(seeds[1].id, ItemRef::new(s, "show1"));
    }

    #[test]
    fn recommendations_skip_watched_titles_repeats_and_thin_rows() {
        let s = ServerId::new();
        let seed = |key: &str, title: &str| Seed { id: ItemRef::new(s, key), title: title.into() };
        let plain = |key: &str| MediaItem::new(ItemRef::new(s, key), ItemKind::Movie, key);
        let mut seen = plain("seen");
        seen.user.played = true;
        let rows = recommendation_rows(
            vec![
                (seed("a", "Alpha"), vec![plain("m1"), seen, plain("Alpha"), plain("m2"), plain("m3")]),
                (seed("b", "Beta"), vec![plain("m1"), plain("m4"), plain("m5")]),
                (seed("c", "Gamma"), vec![plain("m6")]),
            ],
            20,
        );
        assert_eq!(rows.len(), 1, "Beta is left with 2 new titles and Gamma with 1: too thin");
        assert_eq!(rows[0].title, "Because you watched Alpha");
        let keys: Vec<&str> = rows[0].items.iter().map(|i| i.id.key.as_str()).collect();
        assert_eq!(keys, vec!["m1", "m2", "m3"]);
    }

    #[test]
    fn titles_from_two_servers_are_ordered_together() {
        let (a, b) = (ServerId::new(), ServerId::new());
        let mk = |s, t: &str| MediaItem::new(ItemRef::new(s, t), ItemKind::Movie, t);
        let mut items = vec![mk(a, "Zodiac"), mk(b, "alien"), mk(a, "Memento")];
        sort_by_title(&mut items, SortOrder::Ascending);
        assert_eq!(items.iter().map(|i| i.title.as_str()).collect::<Vec<_>>(), vec!["alien", "Memento", "Zodiac"]);
        sort_by_title(&mut items, SortOrder::Descending);
        assert_eq!(items[0].title, "Zodiac");
    }

    #[test]
    fn continue_watching_rows_are_merged_by_recency() {
        let (a, b) = (ServerId::new(), ServerId::new());
        let row = |items| HomeRow { kind: HomeRowKind::ContinueWatching, title: "x".into(), items };
        let rows = merge_rows(
            vec![
                ("A".into(), vec![row(vec![movie(a, "old", None, 1)])]),
                ("B".into(), vec![row(vec![movie(b, "new", None, 5)])]),
            ],
            true,
        );
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].items[0].id.key, "new");
    }
}
