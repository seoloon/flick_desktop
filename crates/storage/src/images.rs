//! Artwork cache on disk, keyed by (image reference, size bucket).
//!
//! Files are content-addressed by a SHA-256 of the key, so cache paths never
//! leak server URLs or tokens. Eviction is LRU by modification time (touched
//! on read), capped in bytes.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::SystemTime;

use oneshot_core::media::{ImageRef, ImageSize};
use oneshot_core::{Error, Result};
use sha2::{Digest, Sha256};

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
    let mut h = Sha256::new();
    h.update(b"avatar\0");
    h.update(url.as_str().as_bytes());
    h.finalize().iter().map(|b| format!("{b:02x}")).collect()
}

impl ImageCache {
    pub fn new(dir: PathBuf, max_bytes: u64) -> Result<Self> {
        std::fs::create_dir_all(&dir).map_err(|e| Error::Storage(e.to_string()))?;
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
        let bytes = std::fs::read(&path).ok()?;
        // Touch for LRU; failure only degrades eviction order.
        let _ = std::fs::File::options().append(true).open(&path).and_then(|f| f.set_modified(SystemTime::now()));
        Some(bytes)
    }

    pub fn put(&self, key: &str, bytes: &[u8]) -> Result<()> {
        let path = self.path(key);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| Error::Storage(e.to_string()))?;
        }
        let tmp = path.with_extension("tmp");
        std::fs::write(&tmp, bytes).and_then(|()| std::fs::rename(&tmp, &path)).map_err(|e| Error::Storage(e.to_string()))
    }

    /// Evicts least-recently-used files until the cache fits its budget.
    /// Returns the number of bytes freed.
    pub fn enforce_limit(&self) -> Result<u64> {
        let mut files: Vec<(PathBuf, u64, SystemTime)> = Vec::new();
        collect(&self.dir, &mut files).map_err(|e| Error::Storage(e.to_string()))?;
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
        std::fs::remove_dir_all(&self.dir).map_err(|e| Error::Storage(e.to_string()))?;
        std::fs::create_dir_all(&self.dir).map_err(|e| Error::Storage(e.to_string()))
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
    fn keys_depend_on_size_and_do_not_leak_tags() {
        let img = ImageRef { item: ItemRef::new(ServerId::new(), "1"), kind: ImageKind::Poster, tag: "/secret?X-Plex-Token=abc".into(), blurhash: None };
        let a = cache_key(&img, ImageSize::Card);
        let b = cache_key(&img, ImageSize::Hero);
        assert_ne!(a, b);
        assert!(!a.contains("abc") && a.len() == 64);
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
