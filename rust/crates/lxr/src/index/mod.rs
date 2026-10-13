//! The index on disk: what is stored, how it is kept current, and the status line.
//!
//! An index is published as one generation. Its pieces (the graph, the stored facts, the
//! search folder) are written beside the live ones and put in place one after the other, and
//! the manifest comes last. The manifest records the size and hash of the graph and of the
//! facts and the build time that the search folder carries, so a reader checks every piece it
//! loads against the manifest it started from. Pieces of two builds, whatever left them side by
//! side, never agree with one manifest and are not used.

pub mod lock;
pub mod manifest;
pub mod refresh;
pub mod status;

use lxr_core::hash64;
use lxr_graph::Graph;
use lxr_rag::SearchIndex;
use manifest::Part;
use std::fs;
use std::path::{Path, PathBuf};

/// The file that lists every indexed file with its size, time and hash.
pub const MANIFEST_FILE: &str = "manifest.bin";
/// The stored facts of every indexed file.
pub const FACTS_FILE: &str = "facts.bin";
/// The code graph.
pub const GRAPH_FILE: &str = "graph.bin";
/// The folder of the search index.
pub const SEARCH_DIR: &str = "search";
/// The file inside the search folder that holds the build time of the build that made it.
pub const SEARCH_TAG: &str = "generation";
/// The folder of the memory bank, inside the index folder.
pub const MEMORY_DIR: &str = "memory";
/// The lock file a refresh holds while it writes.
pub const LOCK_FILE: &str = "refresh.lock";
/// The file whose operating system lock orders the taking over of an expired `refresh.lock`.
pub const GUARD_FILE: &str = "refresh.guard";
/// The file in a memory bank folder whose lock orders the writes of pgr processes.
pub const BANK_LOCK_FILE: &str = "pgr.lock";

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

/// The size and hash of the file at `path`, as a manifest records them.
pub fn part_of(path: &Path) -> std::io::Result<Part> {
    let bytes = fs::read(path)?;
    Ok(Part { len: bytes.len() as u64, hash: hash64(&bytes) })
}

/// True when the file at `path` is the one `part` describes.
pub fn part_matches(path: &Path, part: Part) -> bool {
    part_of(path).map_or(false, |found| found == part)
}

/// The build time that the search folder at `path` carries, if it carries one.
pub fn search_tag(path: &Path) -> Option<u64> {
    fs::read_to_string(path.join(SEARCH_TAG)).ok()?.trim().parse().ok()
}

/// Loads the graph at `path`, and gives it only when the file is the one `part` describes.
/// The file is read into the graph first and checked after: a file that a build replaced in
/// between fails the check instead of being taken on the strength of a check made on the
/// file it replaced.
pub fn load_graph(path: &Path, part: Part) -> Option<Graph> {
    let graph = Graph::load(path).ok()?;
    part_matches(path, part).then_some(graph)
}

/// Opens the search folder at `path`, and gives it only when it carries the build time `id`.
/// As with the graph, the folder is opened first and its tag read after.
pub fn open_search(path: &Path, id: u64) -> Option<SearchIndex> {
    let index = SearchIndex::open(path).ok()?;
    (search_tag(path) == Some(id)).then_some(index)
}
