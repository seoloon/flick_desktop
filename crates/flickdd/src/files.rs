//! File names and the partial file.

use std::io::SeekFrom;
use std::path::{Path, PathBuf};

use tokio::fs::{File, OpenOptions};
use tokio::io::AsyncSeekExt;

/// A name that is safe on every OS: no path separators, no characters Windows
/// refuses, no reserved device names, never empty.
pub fn sanitize_filename(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| if c.is_control() || matches!(c, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*') { '_' } else { c })
        .collect();
    let cleaned = cleaned.trim().trim_matches('.').trim();
    let mut out: String = cleaned.chars().take(150).collect();
    if out.is_empty() {
        out = "download".into();
    }
    let stem = out.split('.').next().unwrap_or("").to_ascii_uppercase();
    let reserved = matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || (stem.len() == 4 && (stem.starts_with("COM") || stem.starts_with("LPT")) && stem.as_bytes()[3].is_ascii_digit());
    if reserved { format!("_{out}") } else { out }
}

/// Containers a downloaded file may keep. The name comes from the server and the
/// finished file is opened by the system: anything else must never keep its extension.
pub const MEDIA_EXTENSIONS: &[&str] = &["mkv", "mp4", "m4v", "avi", "mov", "webm", "ts", "m2ts", "wmv", "flv", "mpg", "mpeg"];

pub fn is_media_extension(path: &Path) -> bool {
    path.extension().and_then(|e| e.to_str()).is_some_and(|e| MEDIA_EXTENSIONS.contains(&e.to_ascii_lowercase().as_str()))
}

/// A safe name for a downloaded video: sanitized, and ending in a known video
/// container (`.mkv` is appended when the server's extension is missing or not one).
pub fn media_filename(name: &str) -> String {
    let name = sanitize_filename(name);
    if is_media_extension(Path::new(&name)) { name } else { format!("{name}.mkv") }
}

/// `dir/name`, or `dir/name (2).ext`, `(3)`…: the first path that does not exist.
pub fn unique_path(dir: &Path, name: &str) -> PathBuf {
    let name = sanitize_filename(name);
    let first = dir.join(&name);
    if !first.exists() {
        return first;
    }
    let (stem, ext) = match name.rfind('.') {
        Some(i) if i > 0 => (&name[..i], &name[i..]),
        _ => (name.as_str(), ""),
    };
    (2..)
        .map(|n| dir.join(format!("{stem} ({n}){ext}")))
        .find(|p| !p.exists())
        .unwrap_or(first)
}

/// Opens the partial file positioned at `offset`, dropping whatever lies beyond:
/// bytes past the saved offset were never accounted for.
pub async fn open_at(path: &Path, offset: u64) -> std::io::Result<File> {
    if let Some(dir) = path.parent() {
        tokio::fs::create_dir_all(dir).await?;
    }
    let mut file = OpenOptions::new().create(true).write(true).truncate(false).open(path).await?;
    file.set_len(offset).await?;
    file.seek(SeekFrom::Start(offset)).await?;
    Ok(file)
}

/// Size of a file, 0 when it does not exist.
pub async fn size_of(path: &Path) -> u64 {
    tokio::fs::metadata(path).await.map_or(0, |m| m.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_made_safe() {
        assert_eq!(sanitize_filename("The Long Night (2021).mkv"), "The Long Night (2021).mkv");
        assert_eq!(sanitize_filename("a/b\\c:d*e?.mkv"), "a_b_c_d_e_.mkv");
        assert_eq!(sanitize_filename("../../etc/passwd"), "_.._etc_passwd");
        assert_eq!(sanitize_filename("  ..  "), "download");
        assert_eq!(sanitize_filename("con.mkv"), "_con.mkv");
        assert_eq!(sanitize_filename("LPT1"), "_LPT1");
        assert_eq!(sanitize_filename("com10.txt"), "com10.txt");
        assert!(sanitize_filename(&"x".repeat(400)).len() <= 150);
    }

    #[test]
    fn only_video_extensions_survive() {
        assert_eq!(media_filename("Movie.MKV"), "Movie.MKV");
        assert_eq!(media_filename("Movie.mp4"), "Movie.mp4");
        assert_eq!(media_filename("setup.exe"), "setup.exe.mkv");
        assert_eq!(media_filename("run.bat"), "run.bat.mkv");
        assert_eq!(media_filename("shortcut.lnk"), "shortcut.lnk.mkv");
        assert_eq!(media_filename("noext"), "noext.mkv");
        assert!(is_media_extension(Path::new("a.webm")) && !is_media_extension(Path::new("a.exe")));
    }

    #[test]
    fn names_never_overwrite() {
        let dir = tempfile::tempdir().unwrap();
        let first = unique_path(dir.path(), "Movie.mkv");
        assert_eq!(first, dir.path().join("Movie.mkv"));
        std::fs::write(&first, b"x").unwrap();
        assert_eq!(unique_path(dir.path(), "Movie.mkv"), dir.path().join("Movie (2).mkv"));
        std::fs::write(dir.path().join("Movie (2).mkv"), b"x").unwrap();
        assert_eq!(unique_path(dir.path(), "Movie.mkv"), dir.path().join("Movie (3).mkv"));
    }

    #[tokio::test]
    async fn open_at_drops_bytes_past_the_offset() {
        use tokio::io::AsyncWriteExt;
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("a.part");
        std::fs::write(&p, b"0123456789").unwrap();
        let mut f = open_at(&p, 4).await.unwrap();
        f.write_all(b"XY").await.unwrap();
        f.sync_all().await.unwrap();
        assert_eq!(std::fs::read(&p).unwrap(), b"0123XY");
    }
}
