//! Which files of a repository are indexed.

use ignore::WalkBuilder;
use lxr_core::Lang;
use std::io::Write;
use std::path::{Path, PathBuf};

/// Folders that hold built, fetched or tool-made files whatever they sit under. Their names
/// are not words that a package or a source folder is called.
const SKIPPED_DIRS: &[&str] = &[
    "node_modules", ".venv", "__pycache__", ".tox", ".mypy_cache", ".pytest_cache", ".ruff_cache", "site-packages",
    "third_party", ".next", ".nuxt", ".cache", "graphify-out", ".codanna", ".semble", ".idea", ".vscode",
];

/// Folders that usually hold build output or fetched code, but are also ordinary names for a
/// package or a source folder (`operations/build/`, `app/env/`, `com/acme/out/`). They are left
/// out at the top of the repository and next to a build manifest, and read anywhere else.
const OUTPUT_DIRS: &[&str] = &["target", "dist", "build", "out", "env", "venv", "vendor", "coverage"];

/// Files that mark a folder as a project, so that an `OUTPUT_DIRS` folder beside one is its output.
const MANIFESTS: &[&str] = &[
    "package.json", "Cargo.toml", "pom.xml", "build.gradle", "build.gradle.kts", "settings.gradle", "go.mod",
    "composer.json", "Gemfile", "setup.py", "setup.cfg", "pyproject.toml", "CMakeLists.txt", "Makefile", "mix.exs",
    "pubspec.yaml", "Package.swift",
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
/// Follows `.gitignore`, leaves out hidden entries, the folders in `SKIPPED_DIRS`, the
/// folders in `OUTPUT_DIRS` that are at the top of `root` or beside a build manifest,
/// minified files, files over the size limit and files whose extension is not a
/// known language. A part of the tree that cannot be read is reported on standard error.
pub fn walk(root: &Path, opts: &WalkOpts) -> Vec<String> {
    let (found, errors) = scan(root, opts);
    for error in &errors {
        // The files under a folder that cannot be read are missing from the result. `walk` has
        // no way to return that, so it says so. [`walk_checked`] returns it.
        say_unread(root, error);
    }
    found
}

/// A part of the tree that a walk could not read, so that the files under it are not in the
/// result.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Unreadable {
    /// The folder (or file) that could not be read: its path from the root with forward slashes,
    /// `.` for the root itself, and the path as the system gave it when it is not under the root.
    pub path: String,
    /// What went wrong, as the system says it: `Access is denied. (os error 5)`.
    pub reason: String,
}

/// What [`walk_checked`] found.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Walked {
    /// The source files, as [`walk`] gives them.
    pub files: Vec<String>,
    /// The parts of the tree that could not be read, in the order the walk met them. Files may
    /// be missing from `files` because of them; a caller that must not mistake such a file for
    /// a deleted one stops here.
    pub unreadable: Vec<Unreadable>,
}

/// Like [`walk`], and also says what could not be read, instead of only printing it.
///
/// A folder that cannot be listed (its permissions forbid it, say), a loop of links and a root
/// that does not exist are returned in `unreadable`, and nothing is printed for them. A problem
/// with an ignore file (a glob that does not parse) drops no source file: it is printed, as
/// [`walk`] prints it, and the walk goes on.
pub fn walk_checked(root: &Path, opts: &WalkOpts) -> Walked {
    let (files, errors) = scan(root, opts);
    let mut unreadable = Vec::new();
    for error in errors {
        if matches!(error, ignore::Error::Partial(_)) {
            say_unread(root, &error);
            continue;
        }
        let path = match error_path(&error) {
            Some(path) => relative(root, path).filter(|r| !r.is_empty()).unwrap_or_else(|| display_path(root, path)),
            None => ".".to_string(),
        };
        let reason = error.io_error().map_or_else(|| error.to_string(), ToString::to_string);
        unreadable.push(Unreadable { path, reason });
    }
    Walked { files, unreadable }
}

/// The walk itself: the sorted source files, and every error the walker met.
fn scan(root: &Path, opts: &WalkOpts) -> (Vec<String>, Vec<ignore::Error>) {
    let mut found = Vec::new();
    let mut errors = Vec::new();
    let walker = WalkBuilder::new(root)
        .hidden(true)
        .git_ignore(true)
        .git_exclude(true)
        .parents(true)
        .require_git(false)
        .filter_entry(|entry| !is_skipped_dir(entry))
        .build();
    for entry in walker {
        let entry = match entry {
            Ok(entry) => entry,
            Err(error) => {
                errors.push(error);
                continue;
            }
        };
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
    (found, errors)
}

/// Prints that part of the tree under `root` could not be read.
fn say_unread(root: &Path, error: &ignore::Error) {
    let _ = writeln!(std::io::stderr(), "lxr-extract: part of {} could not be read: {error}", root.display());
}

/// The path an error is about, when it names one.
fn error_path(error: &ignore::Error) -> Option<&Path> {
    match error {
        ignore::Error::WithPath { path, .. } => Some(path),
        ignore::Error::Loop { child, .. } => Some(child),
        ignore::Error::WithDepth { err, .. } | ignore::Error::WithLineNumber { err, .. } => error_path(err),
        ignore::Error::Partial(errors) => errors.iter().find_map(error_path),
        _ => None,
    }
}

/// `path` for a message when it is not under the root (or is the root): as the system gave it,
/// with forward slashes.
fn display_path(root: &Path, path: &Path) -> String {
    if path == root {
        ".".to_string()
    } else {
        path.to_string_lossy().replace('\\', "/")
    }
}

/// True for a folder that holds built, fetched or tool-made files: one of `SKIPPED_DIRS`, or
/// one of `OUTPUT_DIRS` that is at the top of the repository or beside a build manifest. A
/// virtual environment is also known by the `pyvenv.cfg` it holds, wherever it sits.
fn is_skipped_dir(entry: &ignore::DirEntry) -> bool {
    if entry.depth() == 0 || !entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
        return false;
    }
    let name = entry.file_name().to_string_lossy();
    if SKIPPED_DIRS.contains(&name.as_ref()) {
        return true;
    }
    if !OUTPUT_DIRS.contains(&name.as_ref()) {
        return false;
    }
    let environment = matches!(name.as_ref(), "env" | "venv");
    if environment && entry.path().join("pyvenv.cfg").is_file() {
        return true;
    }
    entry.depth() == 1 || (!environment && entry.path().parent().map_or(false, has_manifest))
}

fn has_manifest(dir: &Path) -> bool {
    MANIFESTS.iter().any(|name| dir.join(name).is_file())
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

    #[test]
    fn walk_reads_source_folders_that_only_share_a_name_with_build_output() {
        let root = std::env::temp_dir().join(format!("lxr-walk-names-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let files = [
            // Source whose folder is called like an output folder.
            "src/pip/_internal/operations/build/wheel.py",
            "app/env/config.py",
            "com/acme/out/Writer.java",
            "lib/vendor/glue.rb",
            // Output at the top of the repository.
            "build/gen.py",
            "dist/bundle.py",
            "target/out.rs",
            "vendor/dep.go",
            // Output beside a build manifest, and folders that are never source.
            "web/package.json",
            "web/dist/app.js",
            "crate/Cargo.toml",
            "crate/target/debug.rs",
            "web/node_modules/x/index.js",
            "web/src/__pycache__/m.py",
            // A virtual environment is known by its marker, wherever it is.
            "tools/venv/pyvenv.cfg",
            "tools/venv/lib/site.py",
            // The same names at the top of the repository.
            "env/pyvenv.cfg",
            "env/lib/site.py",
        ];
        for file in files {
            let path = root.join(file);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(&path, "x = 1\n").unwrap();
        }

        let found = walk(&root, &WalkOpts::default());
        assert_eq!(
            found,
            [
                "app/env/config.py",
                "com/acme/out/Writer.java",
                "lib/vendor/glue.rb",
                "src/pip/_internal/operations/build/wheel.py",
            ]
        );
        fs::remove_dir_all(&root).unwrap();
    }

    /// A folder that cannot be listed for as long as the value lives.
    struct Locked(PathBuf);

    impl Locked {
        /// Takes the right to list `dir` away. `None` when this account can list it anyway (an
        /// administrator), so that the failure cannot be made.
        fn new(dir: &Path) -> Option<Locked> {
            #[cfg(windows)]
            {
                // Deny the right to list the folder to everyone.
                let _ = std::process::Command::new("icacls").arg(dir).args(["/deny", "*S-1-1-0:(RD)"]).output();
            }
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let _ = fs::set_permissions(dir, fs::Permissions::from_mode(0o000));
            }
            let locked = Locked(dir.to_path_buf());
            if fs::read_dir(dir).is_ok() {
                eprintln!("skipped: this account can list a folder it is denied");
                return None;
            }
            Some(locked)
        }
    }

    impl Drop for Locked {
        fn drop(&mut self) {
            #[cfg(windows)]
            {
                let _ = std::process::Command::new("icacls").arg(&self.0).args(["/remove:d", "*S-1-1-0"]).output();
            }
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let _ = fs::set_permissions(&self.0, fs::Permissions::from_mode(0o755));
            }
        }
    }

    #[test]
    fn walk_checked_gives_the_files_that_walk_gives_and_nothing_unreadable_for_a_whole_tree() {
        let root = std::env::temp_dir().join(format!("lxr-walk-checked-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("src/deep")).unwrap();
        fs::write(root.join("src/a.py"), "x = 1\n").unwrap();
        fs::write(root.join("src/deep/b.rs"), "fn b() {}\n").unwrap();
        let walked = walk_checked(&root, &WalkOpts::default());
        assert_eq!(walked.files, ["src/a.py", "src/deep/b.rs"]);
        assert_eq!(walked.files, walk(&root, &WalkOpts::default()));
        assert!(walked.unreadable.is_empty(), "{:?}", walked.unreadable);
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn walk_checked_says_which_folder_it_could_not_read() {
        let root = std::env::temp_dir().join(format!("lxr-walk-locked-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("ok")).unwrap();
        fs::create_dir_all(root.join("src/private")).unwrap();
        fs::write(root.join("ok/a.py"), "x = 1\n").unwrap();
        fs::write(root.join("src/private/secret.py"), "x = 2\n").unwrap();
        fs::write(root.join("src/open.py"), "x = 3\n").unwrap();
        let Some(locked) = Locked::new(&root.join("src/private")) else {
            fs::remove_dir_all(&root).unwrap();
            return;
        };
        let walked = walk_checked(&root, &WalkOpts::default());
        // The files under the folder are not in the answer, and the answer says why.
        assert_eq!(walked.files, ["ok/a.py", "src/open.py"]);
        assert_eq!(walked.unreadable.len(), 1, "{:?}", walked.unreadable);
        assert_eq!(walked.unreadable[0].path, "src/private");
        assert!(!walked.unreadable[0].reason.is_empty());
        // `walk` still gives only the files.
        assert_eq!(walk(&root, &WalkOpts::default()), walked.files);
        drop(locked);
        // Once the folder can be read again, it is.
        let again = walk_checked(&root, &WalkOpts::default());
        assert_eq!(again.files, ["ok/a.py", "src/open.py", "src/private/secret.py"]);
        assert!(again.unreadable.is_empty());
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn a_root_that_does_not_exist_is_unreadable_and_has_no_files() {
        let root = std::env::temp_dir().join(format!("lxr-walk-missing-{}", std::process::id())).join("nowhere");
        let walked = walk_checked(&root, &WalkOpts::default());
        assert!(walked.files.is_empty());
        assert_eq!(walked.unreadable.len(), 1, "{:?}", walked.unreadable);
        assert_eq!(walked.unreadable[0].path, ".");
        assert!(!walked.unreadable[0].reason.is_empty());
        assert!(walk(&root, &WalkOpts::default()).is_empty());
    }

    #[test]
    fn a_problem_in_an_ignore_file_is_not_an_unreadable_folder() {
        let root = std::env::temp_dir().join(format!("lxr-walk-glob-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("a.py"), "x = 1\n").unwrap();
        // A glob that does not parse costs one rule, not a file.
        fs::write(root.join(".gitignore"), "[unclosed\nignored.py\n").unwrap();
        fs::write(root.join("ignored.py"), "x = 2\n").unwrap();
        let walked = walk_checked(&root, &WalkOpts::default());
        assert_eq!(walked.files, ["a.py"]);
        assert!(walked.unreadable.is_empty(), "{:?}", walked.unreadable);
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn walk_reads_a_root_folder_that_is_called_like_build_output() {
        let root = std::env::temp_dir().join(format!("lxr-walk-root-{}", std::process::id())).join("build");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("a.py"), "x = 1\n").unwrap();
        assert_eq!(walk(&root, &WalkOpts::default()), ["a.py"]);
        fs::remove_dir_all(root.parent().unwrap()).unwrap();
    }
}
