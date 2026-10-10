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
