//! The manifest: what the index was built from, so that the next run can tell what changed.
//! It is also the record that ties the pieces of one index together: it is written last,
//! and it names the graph and the stored facts by size and hash and the search folder by the
//! build time, so that a piece left by another build is never taken for one of this index.

use super::manifest_path;
use serde::{Deserialize, Serialize};
use std::io;
use std::path::Path;

/// The number of the index file layout. A different number means a full rebuild. 3 stores
/// every index file packed and losslessly (see `lxr_core::pack`).
pub const FORMAT: u32 = 3;

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

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn sample() -> Manifest {
        Manifest {
            format: FORMAT,
            version: VERSION.to_string(),
            built: 1_760_000_000_123_456_789,
            embeddings: true,
            counts: Counts { files: 200, symbols: 2133, links: 5051, chunks: u64::MAX },
            graph: Part { len: 208_796, hash: u64::MAX },
            facts: Part { len: 264_970, hash: 7 },
            files: (0..200u64)
                .map(|i| Entry {
                    path: format!("pkg/\u{4e2d}m{i}.py"),
                    size: i * 37,
                    mtime: 1_700_000_000_000_000_000 + i,
                    hash: i.wrapping_mul(0x9e37_79b9_7f4a_7c15),
                })
                .collect(),
        }
    }

    fn temp(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("pgr-manifest-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn a_manifest_survives_the_store_field_for_field() {
        let dir = temp("round");
        let manifest = sample();
        manifest.save(&dir).unwrap();
        assert_eq!(&fs::read(manifest_path(&dir)).unwrap()[..4], b"LXZ1");
        let back = Manifest::load(&dir).expect("the manifest reads back");
        assert_eq!((back.format, &back.version, back.built, back.embeddings), (manifest.format, &manifest.version, manifest.built, manifest.embeddings));
        assert_eq!((back.counts, back.graph, back.facts), (manifest.counts, manifest.graph, manifest.facts));
        assert_eq!(back.files, manifest.files);
        assert_eq!(back.generation(), manifest.generation());
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_damaged_other_format_or_other_version_manifest_is_not_given() {
        let dir = temp("damaged");
        let good = {
            sample().save(&dir).unwrap();
            fs::read(manifest_path(&dir)).unwrap()
        };
        for at in (0..good.len()).step_by(7).chain([good.len() - 1]) {
            let mut damaged = good.clone();
            damaged[at] ^= 0x40;
            fs::write(manifest_path(&dir), &damaged).unwrap();
            if let Some(back) = Manifest::load(&dir) {
                // A bit that no decoder reads may change nothing; it never changes a value.
                assert_eq!(back.files, sample().files, "byte {at}");
            }
        }
        fs::write(manifest_path(&dir), &good[..good.len() / 2]).unwrap();
        assert!(Manifest::load(&dir).is_none());
        for (format, version) in [(FORMAT - 1, VERSION.to_string()), (FORMAT, "0.0.0".to_string())] {
            let manifest = Manifest { format, version, ..sample() };
            manifest.save(&dir).unwrap();
            assert!(Manifest::load(&dir).is_none(), "format {format}");
        }
        fs::remove_dir_all(&dir).unwrap();
    }
}
