//! Which files of a repository are indexed.

use ignore::WalkBuilder;
use lxr_core::Lang;
use std::path::{Path, PathBuf};

/// Folders that hold built or fetched code, never the repository's own source.
const SKIPPED_DIRS: &[&str] = &[
    "node_modules", "target", "dist", "build", "out", ".venv", "venv", "env", "__pycache__", ".tox", ".mypy_cache",
    ".pytest_cache", ".ruff_cache", "site-packages", "vendor", "third_party", ".next", ".nuxt", ".cache", "coverage",
    "graphify-out", ".codanna", ".semble", ".idea", ".vscode",
];

/// Limits for [`walk`].
#[derive(Clone, Debug)]
pub struct WalkOpts {
    /// A file larger than this is left out. Generated and minified files are the
    /// usual reason a source file is this big.
    pub max_bytes: u64,
}

impl Default for WalkOpts {
    fn default() -> WalkOpts {
        WalkOpts { max_bytes: 1_500_000 }
    }
}

/// The source files under `root`, as paths from `root` with forward slashes, sorted.
///
/// Follows `.gitignore`, leaves out hidden entries, the folders in `SKIPPED_DIRS`,
/// minified files, files over the size limit and files whose extension is not a
/// known language.
pub fn walk(root: &Path, opts: &WalkOpts) -> Vec<String> {
    let mut found = Vec::new();
    let walker = WalkBuilder::new(root)
        .hidden(true)
        .git_ignore(true)
        .git_exclude(true)
        .parents(true)
        .require_git(false)
        .filter_entry(|entry| {
            let name = entry.file_name().to_string_lossy();
            !(entry.file_type().map(|t| t.is_dir()).unwrap_or(false) && SKIPPED_DIRS.contains(&name.as_ref()))
        })
        .build();
    for entry in walker.flatten() {
        if !entry.file_type().map(|t| t.is_file()).unwrap_or(false) {
            continue;
        }
        let path = entry.path();
        if !is_source(path) || entry.metadata().map(|m| m.len() > opts.max_bytes).unwrap_or(true) {
            continue;
        }
        if let Some(relative) = relative(root, path) {
            found.push(relative);
        }
    }
    found.sort();
    found
}

fn is_source(path: &Path) -> bool {
    let name = path.file_name().map(|n| n.to_string_lossy().to_lowercase()).unwrap_or_default();
    if name.ends_with(".min.js") || name.ends_with(".min.mjs") || name.ends_with(".bundle.js") || name.ends_with(".d.ts") {
        return false;
    }
    path.extension().and_then(|e| e.to_str()).and_then(Lang::from_extension).is_some()
}

fn relative(root: &Path, path: &Path) -> Option<String> {
    let rest: PathBuf = path.strip_prefix(root).ok()?.to_path_buf();
    Some(rest.to_string_lossy().replace('\\', "/"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn walk_keeps_source_and_leaves_out_the_rest() {
        let root = std::env::temp_dir().join(format!("lxr-walk-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        for dir in ["src", "node_modules/pkg", "docs", ".hidden"] {
            fs::create_dir_all(root.join(dir)).unwrap();
        }
        fs::write(root.join("src/a.py"), "x = 1\n").unwrap();
        fs::write(root.join("src/b.rs"), "fn b() {}\n").unwrap();
        fs::write(root.join("src/app.min.js"), "x").unwrap();
        fs::write(root.join("src/ignored.py"), "x = 1\n").unwrap();
        fs::write(root.join("node_modules/pkg/index.js"), "x").unwrap();
        fs::write(root.join("docs/readme.md"), "text").unwrap();
        fs::write(root.join(".hidden/c.py"), "x = 1\n").unwrap();
        fs::write(root.join(".gitignore"), "src/ignored.py\n").unwrap();
        fs::write(root.join("src/big.py"), "x".repeat(200)).unwrap();

        let found = walk(&root, &WalkOpts { max_bytes: 100 });
        assert_eq!(found, ["src/a.py", "src/b.rs"]);
        fs::remove_dir_all(&root).unwrap();
    }
}
