//! The index on disk: what is stored, how it is kept current, and the status line.

pub mod lock;
pub mod manifest;
pub mod refresh;
pub mod status;

use std::path::{Path, PathBuf};

/// The file that lists every indexed file with its size, time and hash.
pub const MANIFEST_FILE: &str = "manifest.bin";
/// The stored facts of every indexed file.
pub const FACTS_FILE: &str = "facts.bin";
/// The code graph.
pub const GRAPH_FILE: &str = "graph.bin";
/// The folder of the search index.
pub const SEARCH_DIR: &str = "search";
/// The folder of the memory bank, inside the index folder.
pub const MEMORY_DIR: &str = "memory";
/// The lock file a refresh holds while it writes.
pub const LOCK_FILE: &str = "refresh.lock";

/// The path of the manifest inside an index folder.
pub fn manifest_path(dir: &Path) -> PathBuf {
    dir.join(MANIFEST_FILE)
}

/// The path of the stored facts inside an index folder.
pub fn facts_path(dir: &Path) -> PathBuf {
    dir.join(FACTS_FILE)
}

/// The path of the graph inside an index folder.
pub fn graph_path(dir: &Path) -> PathBuf {
    dir.join(GRAPH_FILE)
}

/// The path of the search index folder inside an index folder.
pub fn search_path(dir: &Path) -> PathBuf {
    dir.join(SEARCH_DIR)
}

/// True when every part of an index that a query reads is on disk. Only existence is
/// checked here; a damaged file is found when it is loaded.
pub fn parts_present(dir: &Path) -> bool {
    graph_path(dir).is_file() && facts_path(dir).is_file() && search_path(dir).is_dir()
}
