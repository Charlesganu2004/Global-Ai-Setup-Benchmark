//! Where an index lives on disk, and how its parts are written and read.

use serde::{de::DeserializeOwned, Serialize};
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

/// The folder that holds every repository's index.
///
/// `LXR_HOME` when it is set, otherwise `.local-index/rust` in the home directory.
pub fn index_home() -> PathBuf {
    if let Some(dir) = std::env::var_os("LXR_HOME").filter(|v| !v.is_empty()) {
        return PathBuf::from(dir);
    }
    let home = std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    home.join(".local-index").join("rust")
}

/// The root of the repository that `start` is in: the nearest folder, going up,
/// that holds a `.git` entry. `start` itself when there is none.
pub fn find_root(start: &Path) -> PathBuf {
    let start = fs::canonicalize(start).unwrap_or_else(|_| start.to_path_buf());
    let mut at: &Path = &start;
    loop {
        if at.join(".git").exists() {
            return plain(at);
        }
        match at.parent() {
            Some(up) => at = up,
            None => return plain(&start),
        }
    }
}

/// A name for the repository's index folder: its folder name, then a hash of its
/// full path so that two checkouts with one name do not share an index.
pub fn repo_key(root: &Path) -> String {
    let full = plain(root).to_string_lossy().replace('\\', "/").to_lowercase();
    let name: String = root
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "root".to_string())
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.' { c } else { '_' })
        .collect();
    format!("{}-{:012x}", name, hash64(full.as_bytes()) & 0xffff_ffff_ffff)
}

/// The name of the repository a checkout belongs to, for what checkouts of one repository share
/// (the notes of a shared bank). It is [`repo_key`] of the checkout, except in a linked git
/// worktree, where it is that of the checkout whose `.git` folder the worktree points into, so
/// that the worktrees of one repository are one repository. A clone has a `.git` folder of its
/// own and is another repository.
pub fn repo_identity(root: &Path) -> String {
    repo_key(&owner_checkout(root).unwrap_or_else(|| root.to_path_buf()))
}

/// The checkout that owns the `.git` folder a linked worktree points into: the parent of that
/// folder, or the folder itself when it is not named `.git` (a bare repository). `None` when
/// `root` is not a linked worktree, or git's own files do not say where its owner is.
fn owner_checkout(root: &Path) -> Option<PathBuf> {
    let link = fs::read_to_string(root.join(".git")).ok()?;
    let gitdir = link.lines().find_map(|line| line.strip_prefix("gitdir:"))?.trim();
    let gitdir = root.join(gitdir);
    let common = fs::read_to_string(gitdir.join("commondir")).ok()?;
    let common = plain(&fs::canonicalize(gitdir.join(common.trim())).ok()?);
    if common.file_name()? == ".git" {
        common.parent().map(Path::to_path_buf)
    } else {
        Some(common)
    }
}

/// The folder that holds the index of the repository rooted at `root`.
pub fn index_dir(root: &Path) -> PathBuf {
    index_home().join(repo_key(root))
}

/// Writes `value` to `path`, replacing the file in one step so that a reader never
/// sees half of it.
pub fn save<T: Serialize>(path: &Path, value: &T) -> io::Result<()> {
    let bytes = postcard::to_stdvec(value).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    write_atomic(path, &bytes)
}

/// Reads a value written by [`save`].
pub fn load<T: DeserializeOwned>(path: &Path) -> io::Result<T> {
    let bytes = fs::read(path)?;
    postcard::from_bytes(&bytes).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
}

/// Writes bytes to `path` through a temporary file beside it.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    // The process id keeps two processes apart, the counter two threads of one process.
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let turn = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let mut temp = path.as_os_str().to_owned();
    temp.push(format!(".{}.{}.tmp", std::process::id(), turn));
    let temp = PathBuf::from(temp);
    {
        let mut file = fs::File::create(&temp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
    }
    match fs::rename(&temp, path) {
        Ok(()) => Ok(()),
        Err(error) => {
            let _ = fs::remove_file(&temp);
            Err(error)
        }
    }
}

/// A fast 64-bit hash of some bytes. Stable across runs and machines.
pub fn hash64(bytes: &[u8]) -> u64 {
    xxhash_rust::xxh3::xxh3_64(bytes)
}

/// A rough count of the model tokens in a piece of text: four characters to a token.
pub fn est_tokens(text: &str) -> usize {
    (text.len() + 3) / 4
}

/// On Windows, `canonicalize` returns a `\\?\` path. This gives the ordinary form back.
fn plain(path: &Path) -> PathBuf {
    let text = path.to_string_lossy();
    match text.strip_prefix(r"\\?\") {
        Some(rest) if !rest.starts_with("UNC") => PathBuf::from(rest),
        _ => path.to_path_buf(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn save_then_load_gives_the_value_back() {
        let dir = std::env::temp_dir().join(format!("lxr-core-test-{}", std::process::id()));
        let path = dir.join("nested").join("value.bin");
        let value = (vec!["a".to_string(), "b".to_string()], 42u32);
        save(&path, &value).unwrap();
        let back: (Vec<String>, u32) = load(&path).unwrap();
        assert_eq!(back, value);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn repo_key_differs_for_two_folders_with_one_name() {
        let a = repo_key(Path::new("/one/project"));
        let b = repo_key(Path::new("/two/project"));
        assert!(a.starts_with("project-") && b.starts_with("project-"));
        assert_ne!(a, b);
    }

    #[test]
    fn a_linked_worktree_is_named_after_the_checkout_that_owns_its_git_folder() {
        let base = std::env::temp_dir().join(format!("lxr-core-worktree-{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        let (main, wt, clone) = (base.join("main"), base.join("wt"), base.join("clone"));
        let admin = main.join(".git").join("worktrees").join("wt");
        for dir in [&admin, &wt, &clone.join(".git")] {
            fs::create_dir_all(dir).unwrap();
        }
        // Git writes the way back to the common folder relative to the administrative one.
        fs::write(admin.join("commondir"), "../..\n").unwrap();
        fs::write(wt.join(".git"), format!("gitdir: {}\n", admin.display())).unwrap();
        assert_eq!(repo_identity(&wt), repo_key(&main));
        assert_eq!(repo_identity(&main), repo_key(&main));
        assert_eq!(repo_identity(&clone), repo_key(&clone));
        assert_ne!(repo_identity(&clone), repo_identity(&main));
        // A `.git` file that points nowhere leaves the checkout named after itself.
        fs::write(wt.join(".git"), "gitdir: nowhere\n").unwrap();
        assert_eq!(repo_identity(&wt), repo_key(&wt));
        fs::remove_dir_all(&base).unwrap();
    }

    #[test]
    fn token_estimate_rounds_up() {
        assert_eq!(est_tokens(""), 0);
        assert_eq!(est_tokens("abc"), 1);
        assert_eq!(est_tokens("abcde"), 2);
    }
}
