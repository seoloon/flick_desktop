//! Where the ffmpeg executable is.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

pub(crate) const NAME: &str = if cfg!(windows) { "ffmpeg.exe" } else { "ffmpeg" };

/// The `ONESHOT_FFMPEG` file, else `ffmpeg` in one of `dirs` (the folders libmpv is searched in), else on the `PATH`.
pub fn locate(dirs: &[PathBuf]) -> Option<PathBuf> {
    locate_in(std::env::var_os("ONESHOT_FFMPEG"), dirs, std::env::var_os("PATH"))
}

pub(crate) fn locate_in(env: Option<OsString>, dirs: &[PathBuf], path: Option<OsString>) -> Option<PathBuf> {
    let env = env.map(PathBuf::from).filter(|p| p.is_file());
    let in_dirs = || dirs.iter().map(|d| d.join(NAME)).find(|p| p.is_file());
    let on_path = || path.as_deref().and_then(|p| std::env::split_paths(p).map(|d| d.join(NAME)).find(|p: &PathBuf| Path::is_file(p)));
    env.or_else(in_dirs).or_else(on_path)
}

/// Whether `ffmpeg -filters` output lists a filter called `name`.
pub(crate) fn lists_filter(listing: &str, name: &str) -> bool {
    listing.lines().any(|l| l.split_whitespace().nth(1) == Some(name))
}

/// Whether this ffmpeg has the filter (Homebrew's has no `subtitles`: it is built without libass).
pub fn has_filter(ffmpeg: &Path, name: &str) -> bool {
    let mut cmd = std::process::Command::new(ffmpeg);
    cmd.args(["-hide_banner", "-filters"]).stdin(std::process::Stdio::null()).stderr(std::process::Stdio::null());
    #[cfg(windows)]
    std::os::windows::process::CommandExt::creation_flags(&mut cmd, 0x0800_0000); // CREATE_NO_WINDOW
    cmd.output().is_ok_and(|o| lists_filter(&String::from_utf8_lossy(&o.stdout), name))
}

/// Whether the subtitle can be burnt into the picture: a bitmap is overlaid, a text one needs the
/// `subtitles` filter (`text_ok`).
pub fn burnable(subtitle: &oneshot_core::stream::SubtitleStream, text_ok: bool) -> bool {
    use oneshot_core::stream::SubtitleFormat::{Dvb, Pgs, VobSub};
    text_ok || matches!(subtitle.format, Pgs | VobSub | Dvb)
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;
    use std::fs;

    use super::*;

    fn fake(dir: &std::path::Path) -> std::path::PathBuf {
        let p = dir.join(NAME);
        fs::write(&p, b"").unwrap();
        p
    }

    #[test]
    fn the_env_var_wins_then_the_folders_then_the_path() {
        let (a, b, c) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        let (in_a, in_b, in_c) = (fake(a.path()), fake(b.path()), fake(c.path()));
        let dirs = vec![a.path().to_path_buf()];
        let path = Some(std::env::join_paths([c.path()]).unwrap());
        assert_eq!(locate_in(Some(in_b.clone().into_os_string()), &dirs, path.clone()), Some(in_b));
        assert_eq!(locate_in(None, &dirs, path.clone()), Some(in_a));
        assert_eq!(locate_in(None, &[], path), Some(in_c));
    }

    #[test]
    fn an_env_var_to_a_missing_file_is_ignored() {
        assert_eq!(locate_in(Some(OsString::from("/nope/ffmpeg")), &[], None), None);
    }
}

#[cfg(test)]
mod filter_tests {
    use super::*;
    use oneshot_core::stream::{SubtitleFormat, SubtitleStream};

    const LISTING: &str = "Filters:\n  T.. = Timeline support\n ... overlay  VV->V  Overlay a video source on top of the input.\n T.. subtitles  V->V  Render text subtitles onto input video using the libass library.\n .. yadif  V->V  Deinterlace.\n";

    #[test]
    fn a_filter_is_found_by_its_exact_name_in_ffmpegs_listing() {
        assert!(lists_filter(LISTING, "subtitles"));
        assert!(lists_filter(LISTING, "overlay"));
        assert!(!lists_filter(LISTING, "subtitle"));
        assert!(!lists_filter(" ... overlay  VV->V  Overlay\n", "subtitles"));
    }

    fn sub(format: SubtitleFormat) -> SubtitleStream {
        SubtitleStream { index: 2, format, language: None, title: None, forced: false, hearing_impaired: false, is_default: false, external: false, delivery_path: None }
    }

    #[test]
    fn a_text_subtitle_is_only_burnable_with_the_subtitles_filter_and_a_bitmap_always() {
        assert!(!burnable(&sub(SubtitleFormat::Srt), false));
        assert!(burnable(&sub(SubtitleFormat::Srt), true));
        assert!(burnable(&sub(SubtitleFormat::Pgs), false));
    }
}
