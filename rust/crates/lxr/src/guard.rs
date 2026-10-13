//! The folders pgr refuses to index: the home folder or one above it, and any folder
//! that holds a file named `.local-index-skip`.

use crate::fail::{Fail, Res};
use std::path::{Path, PathBuf};

/// The name of the file that switches pgr off for the folder that holds it.
pub const SKIP_FILE: &str = ".local-index-skip";

/// The user's home folder, from `USERPROFILE` or `HOME`.
fn home_dir() -> Option<PathBuf> {
    std::env::var_os("USERPROFILE")
        .filter(|v| !v.is_empty())
        .or_else(|| std::env::var_os("HOME").filter(|v| !v.is_empty()))
        .map(PathBuf::from)
}

/// A path as a lower-case string with forward slashes and no trailing slash.
fn normal(path: &Path) -> String {
    let text = path.to_string_lossy();
    let text = text.strip_prefix(r"\\?\").unwrap_or(&text);
    text.replace('\\', "/").trim_end_matches('/').to_lowercase()
}

/// True when `root` is `home` or a folder above it.
fn is_home_or_above(root: &Path, home: &Path) -> bool {
    let root = normal(root);
    let home = normal(&std::fs::canonicalize(home).unwrap_or_else(|_| home.to_path_buf()));
    home == root || home.starts_with(&format!("{root}/"))
}

/// Gives `Fail::Off` when pgr must not index `root`. The count of source files is checked
/// by the walk, not here.
pub fn check(root: &Path, name: &str) -> Res<()> {
    if let Some(home) = home_dir() {
        if is_home_or_above(root, &home) {
            return Err(Fail::Off(format!("off for {name} (the home folder or a folder above it)")));
        }
    }
    if root.join(SKIP_FILE).exists() {
        return Err(Fail::Off(format!("off for {name} (it holds a {SKIP_FILE} file)")));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_home_folder_and_its_parents_are_refused() {
        let home = Path::new("C:/Users/Someone");
        assert!(is_home_or_above(Path::new("C:/Users/Someone"), home));
        assert!(is_home_or_above(Path::new("C:/Users"), home));
        assert!(is_home_or_above(Path::new("C:/"), home));
        assert!(!is_home_or_above(Path::new("C:/Users/Someone/project"), home));
        assert!(!is_home_or_above(Path::new("C:/Users/Someone2"), home));
    }
}
