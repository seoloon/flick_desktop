//! Cross-server merging rules.

use oneshot_core::media::MediaItem;
use oneshot_core::query::{HomeRow, HomeRowKind};

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
