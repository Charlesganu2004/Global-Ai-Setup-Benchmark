//! The manifest: what the index was built from, so that the next run can tell what changed.
//! It is also the record that ties the pieces of one index together: it is written last,
//! and it names the graph and the stored facts by size and hash and the search folder by the
//! build time, so that a piece left by another build is never taken for one of this index.

use super::manifest_path;
use serde::{Deserialize, Serialize};
use std::io;
use std::path::Path;

/// The number of the index file layout. A different number means a full rebuild.
pub const FORMAT: u32 = 2;

/// The version of this program. An index written by another version is rebuilt.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// One indexed file as it was when the index was built.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    /// Path from the repository root, with forward slashes.
    pub path: String,
    /// Size in bytes.
    pub size: u64,
    /// Modified time in nanoseconds since the Unix epoch.
    pub mtime: u64,
    /// Hash of the bytes of the file.
    pub hash: u64,
}

/// Counts shown by the status line, kept so that `pgr status` reads only this file.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Counts {
    pub files: u64,
    pub symbols: u64,
    pub links: u64,
    pub chunks: u64,
}

/// The size and hash of a file as the build that wrote it left it.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Part {
    pub len: u64,
    pub hash: u64,
}

/// Which build an index belongs to, and what the files of that build must look like. The
/// parts of an index are used together only when they all agree with one generation.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Generation {
    /// The build time, which is unique to the build. 0 when no index is known.
    pub id: u64,
    pub graph: Part,
    pub facts: Part,
}

/// What the index was built from.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Manifest {
    pub format: u32,
    pub version: String,
    /// When the index was last built, in nanoseconds since the Unix epoch. This is the
    /// number of the generation: every build has a later one than the build before it.
    pub built: u64,
    /// True when the search index holds embedding vectors.
    pub embeddings: bool,
    pub counts: Counts,
    /// Size and hash of `graph.bin`.
    pub graph: Part,
    /// Size and hash of `facts.bin`.
    pub facts: Part,
    /// Every indexed file, sorted by path.
    pub files: Vec<Entry>,
}

impl Manifest {
    /// Reads the manifest of the index in `dir`. `None` when there is none, when it is
    /// damaged, or when it was written with another format or by another version.
    pub fn load(dir: &Path) -> Option<Manifest> {
        let manifest: Manifest = lxr_core::load(&manifest_path(dir)).ok()?;
        if manifest.format != FORMAT || manifest.version != VERSION {
            return None;
        }
        Some(manifest)
    }

    /// Writes the manifest into `dir`, replacing the old one in one step.
    pub fn save(&self, dir: &Path) -> io::Result<()> {
        lxr_core::save(&manifest_path(dir), self)
    }

    /// The generation this manifest describes.
    pub fn generation(&self) -> Generation {
        Generation { id: self.built, graph: self.graph, facts: self.facts }
    }
}
