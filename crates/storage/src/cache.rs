//! Metadata cache (SQLite). Servers remain the source of truth: entries have
//! a TTL, are served stale-while-revalidate by the catalogue layer, and are
//! invalidated on every mutation (played/favourite/progress).

use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use oneshot_core::{Error, Result};
use parking_lot::Mutex;
use rusqlite::{Connection, OptionalExtension, params};
use serde::Serialize;
use serde::de::DeserializeOwned;

fn now() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64)
}

fn db_err(e: rusqlite::Error) -> Error {
    Error::Storage(oneshot_core::codes::STO_CACHE.tag(format!("The metadata cache failed ({e}). Clear it in Settings › Network & Cache.")))
}

#[derive(Debug)]
pub struct Cached<T> {
    pub value: T,
    /// Still within its TTL.
    pub fresh: bool,
    pub age_secs: i64,
}

#[derive(Debug)]
pub struct MetadataCache {
    db: Mutex<Connection>,
}

impl MetadataCache {
    pub fn open(path: &Path) -> Result<Self> {
        let db = Connection::open(path).map_err(db_err)?;
        db.execute_batch(
            "PRAGMA journal_mode=WAL;
             PRAGMA synchronous=NORMAL;
             CREATE TABLE IF NOT EXISTS entries (
                key TEXT PRIMARY KEY,
                server TEXT NOT NULL,
                value BLOB NOT NULL,
                fetched_at INTEGER NOT NULL,
                ttl INTEGER NOT NULL
             );
             CREATE INDEX IF NOT EXISTS entries_server ON entries(server);",
        )
        .map_err(db_err)?;
        Ok(Self { db: Mutex::new(db) })
    }

    pub fn in_memory() -> Result<Self> {
        Self::open(Path::new(":memory:"))
    }

    pub fn get<T: DeserializeOwned>(&self, key: &str) -> Result<Option<Cached<T>>> {
        let row: Option<(Vec<u8>, i64, i64)> = self
            .db
            .lock()
            .query_row("SELECT value, fetched_at, ttl FROM entries WHERE key = ?1", params![key], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?))
            })
            .optional()
            .map_err(db_err)?;
        let Some((bytes, fetched_at, ttl)) = row else { return Ok(None) };
        match serde_json::from_slice(&bytes) {
            Ok(value) => {
                let age = now() - fetched_at;
                Ok(Some(Cached { value, fresh: age < ttl, age_secs: age }))
            }
            // Schema changed between versions: treat as a miss.
            Err(_) => Ok(None),
        }
    }

    pub fn put<T: Serialize>(&self, server: &str, key: &str, value: &T, ttl_secs: u32) -> Result<()> {
        let bytes = serde_json::to_vec(value).map_err(|e| Error::Storage(oneshot_core::codes::STO_CACHE.tag(format!("The metadata cache could not store a result ({e})."))))?;
        self.db
            .lock()
            .execute(
                "INSERT INTO entries (key, server, value, fetched_at, ttl) VALUES (?1, ?2, ?3, ?4, ?5)
                 ON CONFLICT(key) DO UPDATE SET value = excluded.value, fetched_at = excluded.fetched_at, ttl = excluded.ttl",
                params![key, server, bytes, now(), i64::from(ttl_secs)],
            )
            .map_err(db_err)?;
        Ok(())
    }

    /// Drops every entry whose key starts with `prefix`.
    pub fn invalidate_prefix(&self, prefix: &str) -> Result<usize> {
        let pattern = format!("{}%", prefix.replace('%', "\\%").replace('_', "\\_"));
        self.db.lock().execute("DELETE FROM entries WHERE key LIKE ?1 ESCAPE '\\'", params![pattern]).map_err(db_err)
    }

    pub fn invalidate_server(&self, server: &str) -> Result<usize> {
        self.db.lock().execute("DELETE FROM entries WHERE server = ?1", params![server]).map_err(db_err)
    }

    /// Removes entries older than `max_age_secs` regardless of TTL.
    pub fn purge_older_than(&self, max_age_secs: i64) -> Result<usize> {
        self.db.lock().execute("DELETE FROM entries WHERE fetched_at < ?1", params![now() - max_age_secs]).map_err(db_err)
    }

    pub fn clear(&self) -> Result<()> {
        self.db.lock().execute("DELETE FROM entries", []).map(drop).map_err(db_err)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn put_get_ttl_and_invalidation() {
        let c = MetadataCache::in_memory().unwrap();
        c.put("s1", "s1:home", &vec![1, 2, 3], 300).unwrap();
        c.put("s1", "s1:item:42", &"x", 0).unwrap();
        c.put("s2", "s2:home", &"y", 300).unwrap();

        let home: Cached<Vec<i32>> = c.get("s1:home").unwrap().unwrap();
        assert!(home.fresh);
        assert_eq!(home.value, vec![1, 2, 3]);
        assert!(!c.get::<String>("s1:item:42").unwrap().unwrap().fresh, "ttl 0 is immediately stale");

        assert_eq!(c.invalidate_prefix("s1:item:").unwrap(), 1);
        assert_eq!(c.invalidate_server("s1").unwrap(), 1);
        assert!(c.get::<String>("s2:home").unwrap().is_some());
    }

    #[test]
    fn like_wildcards_in_keys_are_escaped() {
        let c = MetadataCache::in_memory().unwrap();
        c.put("s", "a_b", &1, 60).unwrap();
        c.put("s", "axb", &1, 60).unwrap();
        assert_eq!(c.invalidate_prefix("a_").unwrap(), 1);
    }
}
