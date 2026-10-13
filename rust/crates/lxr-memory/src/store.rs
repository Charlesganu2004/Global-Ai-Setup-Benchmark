//! The caches: the vector matrix (`vectors.bin`) and the index snapshot (`index.bin`).
//! Both are rebuilt from `notes.jsonl` when they are missing, damaged or older than it.

use crate::entities::{EntityRec, EntityTable};
use crate::graph::Graph;
use crate::Note;
use lxr_text::{code_tokens, Bm25, Vectors};
use serde::{Deserialize, Serialize};
use std::fs;
use std::io;
use std::path::Path;

/// File name of the log of notes.
pub(crate) const LOG_FILE: &str = "notes.jsonl";
/// File name of the vector matrix.
pub(crate) const VECTORS_FILE: &str = "vectors.bin";
/// File name of the index snapshot.
pub(crate) const INDEX_FILE: &str = "index.bin";

/// Marks the first and last field of a snapshot, so that a file of another kind, a cut file
/// and a file of an older layout are all refused.
pub(crate) const MAGIC: u64 = 0x4c58_524d_454d_0003;

/// The words of a note that the keyword search indexes: text, context, tags and entities.
pub(crate) fn note_tokens(note: &Note) -> Vec<String> {
    let mut tokens = code_tokens(&note.text);
    tokens.extend(code_tokens(&note.context));
    for tag in &note.tags {
        tokens.extend(code_tokens(tag));
    }
    for entity in &note.entities {
        tokens.extend(code_tokens(entity));
    }
    tokens
}

/// A BM25 index over all slots; a forgotten note is an empty document.
pub(crate) fn build_bm25(slots: &[Option<Note>]) -> Bm25 {
    Bm25::build(slots.iter().map(|slot| slot.as_ref().map(note_tokens).unwrap_or_default()))
}

/// The dot product of two slices of equal length.
pub(crate) fn dot(a: &[f32], b: &[f32]) -> f32 {
    let mut lanes = [0f32; 8];
    let (ac, bc) = (a.chunks_exact(8), b.chunks_exact(8));
    let (ar, br) = (ac.remainder(), bc.remainder());
    for (x, y) in ac.zip(bc) {
        for i in 0..8 {
            lanes[i] += x[i] * y[i];
        }
    }
    let mut sum: f32 = lanes.iter().sum();
    for (x, y) in ar.iter().zip(br) {
        sum += x * y;
    }
    sum
}

/// The embedding of every note, by slot. The part that was written to `vectors.bin` is
/// memory-mapped; rows added since live in memory until the next flush.
#[derive(Default)]
pub(crate) struct VecStore {
    /// Length of a vector; 0 while no vector exists.
    pub dim: usize,
    mapped: Option<Vectors>,
    mapped_rows: usize,
    /// Rows from `mapped_rows` on, one after the other.
    tail: Vec<f32>,
    /// Whether the note in each slot has a vector.
    pub have: Vec<bool>,
}

impl VecStore {
    /// A store of vectors of length `dim` where no note has a vector yet.
    pub fn empty(dim: usize, slots: usize) -> VecStore {
        VecStore { dim, have: vec![false; slots], ..VecStore::default() }
    }

    /// Maps `path` as the matrix of a store with the given `have` flags. Fails when the file
    /// does not match: another dimension or another number of rows.
    pub fn open(path: &Path, dim: usize, have: Vec<bool>) -> io::Result<VecStore> {
        let vectors = Vectors::open(path)?;
        if vectors.dim() != dim || vectors.len() != have.len() {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "the vector file does not match the index"));
        }
        Ok(VecStore { dim, mapped_rows: vectors.len(), mapped: Some(vectors), tail: Vec::new(), have })
    }

    /// Room for `slots` notes.
    pub fn grow(&mut self, slots: usize) {
        if self.have.len() < slots {
            self.have.resize(slots, false);
        }
    }

    /// True when at least one note has a vector.
    pub fn any(&self) -> bool {
        self.have.iter().any(|&h| h)
    }

    /// The vector of the note in `slot`, if it has one.
    pub fn row(&self, slot: u32) -> Option<&[f32]> {
        let i = slot as usize;
        if self.dim == 0 || !*self.have.get(i)? {
            return None;
        }
        if i < self.mapped_rows {
            let row = self.mapped.as_ref()?.row(slot);
            return if row.len() == self.dim { Some(row) } else { None };
        }
        let start = (i - self.mapped_rows) * self.dim;
        self.tail.get(start..start + self.dim)
    }

    /// Stores the vector of the note in `slot`. A vector of the wrong length is ignored.
    pub fn set(&mut self, slot: u32, vector: &[f32]) {
        if self.dim == 0 || vector.len() != self.dim {
            return;
        }
        let i = slot as usize;
        self.grow(i + 1);
        if i < self.mapped_rows {
            self.materialize();
        }
        let at = i - self.mapped_rows;
        let end = (at + 1) * self.dim;
        if self.tail.len() < end {
            self.tail.resize(end, 0.0);
        }
        self.tail[at * self.dim..end].copy_from_slice(vector);
        self.have[i] = true;
    }

    /// The note in `slot` has no vector any more.
    pub fn clear(&mut self, slot: u32) {
        if let Some(flag) = self.have.get_mut(slot as usize) {
            *flag = false;
        }
    }

    /// Drops every vector and starts again with vectors of length `dim`.
    pub fn reset(&mut self, dim: usize) {
        let slots = self.have.len();
        *self = VecStore::empty(dim, slots);
    }

    /// Calls `keep(slot, dot product)` for every note that has a vector.
    pub fn scan(&self, query: &[f32], mut keep: impl FnMut(u32, f32)) {
        if self.dim == 0 || query.len() != self.dim {
            return;
        }
        for (i, &has) in self.have.iter().enumerate() {
            if has {
                if let Some(row) = self.row(i as u32) {
                    keep(i as u32, dot(query, row));
                }
            }
        }
    }

    /// Copies the mapped rows into memory, so that the file can be replaced.
    fn materialize(&mut self) {
        if let Some(mapped) = self.mapped.take() {
            let mut all = Vec::with_capacity(self.mapped_rows * self.dim + self.tail.len());
            for row in 0..self.mapped_rows {
                all.extend_from_slice(mapped.row(row as u32));
            }
            all.extend_from_slice(&self.tail);
            self.tail = all;
            self.mapped_rows = 0;
        }
    }

    /// Writes the matrix to `path` (or removes the file when no note has a vector) and maps
    /// it again.
    pub fn write(&mut self, path: &Path) -> io::Result<()> {
        if self.dim == 0 || !self.any() {
            self.materialize();
            return match fs::remove_file(path) {
                Err(e) if e.kind() != io::ErrorKind::NotFound => Err(e),
                _ => Ok(()),
            };
        }
        let slots = self.have.len();
        self.materialize();
        self.tail.resize(slots * self.dim, 0.0);
        let rows: Vec<Vec<f32>> = (0..slots)
            .map(|i| {
                let row = &self.tail[i * self.dim..(i + 1) * self.dim];
                if self.have[i] {
                    row.to_vec()
                } else {
                    vec![0.0; self.dim]
                }
            })
            .collect();
        Vectors::write(path, self.dim, &rows)?;
        if let Ok(vectors) = Vectors::open(path) {
            if vectors.len() == slots && vectors.dim() == self.dim {
                self.mapped_rows = slots;
                self.mapped = Some(vectors);
                self.tail = Vec::new();
            }
        }
        Ok(())
    }
}

/// Bytes that serde writes as one run instead of one number at a time, so that a large
/// block is read back with one copy.
mod fast_bytes {
    use serde::de::{self, SeqAccess, Visitor};
    use serde::{Deserializer, Serializer};
    use std::fmt;

    pub fn serialize<S: Serializer>(bytes: &Vec<u8>, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_bytes(bytes)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Vec<u8>, D::Error> {
        struct Bytes;
        impl<'de> Visitor<'de> for Bytes {
            type Value = Vec<u8>;

            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("a run of bytes")
            }

            fn visit_bytes<E: de::Error>(self, v: &[u8]) -> Result<Vec<u8>, E> {
                Ok(v.to_vec())
            }

            fn visit_byte_buf<E: de::Error>(self, v: Vec<u8>) -> Result<Vec<u8>, E> {
                Ok(v)
            }

            fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Vec<u8>, A::Error> {
                let mut out = Vec::new();
                while let Some(byte) = seq.next_element()? {
                    out.push(byte);
                }
                Ok(out)
            }
        }
        deserializer.deserialize_byte_buf(Bytes)
    }
}

/// A run of bytes inside the snapshot.
#[derive(Serialize, Deserialize, Default)]
pub(crate) struct Blob(#[serde(with = "fast_bytes")] pub Vec<u8>);

/// Notes in one block of the snapshot. Blocks are encoded and decoded on their own, so that
/// opening a bank can decode them side by side.
const NOTES_PER_BLOCK: usize = 512;

/// What `index.bin` holds. Each part is encoded on its own (postcard) and stored as a block
/// of bytes: a reader copies the blocks out of the file at once and decodes them in parallel.
#[derive(Serialize, Deserialize)]
struct Snapshot {
    magic: u64,
    /// Length in bytes of `notes.jsonl` when the snapshot was taken.
    log_len: u64,
    next_seq: u64,
    dim: u32,
    /// Number of slots, forgotten notes included.
    slots: u64,
    /// The slots in blocks of [`NOTES_PER_BLOCK`].
    notes: Vec<Blob>,
    /// One byte per slot: 1 when the note has a vector.
    have: Blob,
    bm25: Blob,
    /// The entity records and the co-occurrence counts.
    entities: Blob,
    graph: Blob,
    end: u64,
}

/// The snapshot as it is read: the blocks stay in the buffer the file was read into.
#[derive(Deserialize)]
struct SnapshotRead<'a> {
    magic: u64,
    log_len: u64,
    next_seq: u64,
    dim: u32,
    slots: u64,
    #[serde(borrow)]
    notes: Vec<&'a [u8]>,
    have: &'a [u8],
    bm25: &'a [u8],
    entities: &'a [u8],
    graph: &'a [u8],
    end: u64,
}

/// Everything a snapshot holds, decoded.
pub(crate) struct Loaded {
    pub next_seq: u64,
    pub dim: usize,
    pub slots: Vec<Option<Note>>,
    pub have: Vec<bool>,
    pub bm25: Bm25,
    pub ents: EntityTable,
    pub graph: Graph,
}

/// What to write into a snapshot.
pub(crate) struct Parts<'a> {
    pub log_len: u64,
    pub next_seq: u64,
    pub dim: usize,
    pub slots: &'a [Option<Note>],
    pub have: &'a [bool],
    pub bm25: &'a Bm25,
    pub ents: &'a EntityTable,
    pub graph: &'a Graph,
}

fn invalid<E: std::error::Error + Send + Sync + 'static>(e: E) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, e)
}

/// Writes the snapshot to `path`, replacing the file in one step.
pub(crate) fn save_snapshot(path: &Path, parts: &Parts) -> io::Result<()> {
    let mut notes = Vec::new();
    for block in parts.slots.chunks(NOTES_PER_BLOCK) {
        notes.push(Blob(postcard::to_stdvec(block).map_err(invalid)?));
    }
    let (records, cooc) = parts.ents.to_parts();
    let snapshot = Snapshot {
        magic: MAGIC,
        log_len: parts.log_len,
        next_seq: parts.next_seq,
        dim: parts.dim as u32,
        slots: parts.slots.len() as u64,
        notes,
        have: Blob(parts.have.iter().map(|&h| u8::from(h)).collect()),
        bm25: Blob(postcard::to_stdvec(parts.bm25).map_err(invalid)?),
        entities: Blob(postcard::to_stdvec(&(records, cooc)).map_err(invalid)?),
        graph: Blob(parts.graph.to_bytes(parts.slots.len())),
        end: MAGIC,
    };
    let bytes = postcard::to_stdvec(&snapshot).map_err(invalid)?;
    lxr_core::store::write_atomic(path, &bytes)
}

/// Reads the snapshot at `path` if it is whole and was taken for a log of `log_len` bytes.
/// The blocks are decoded on several threads at once. `None` for anything that does not fit.
pub(crate) fn load_snapshot(path: &Path, log_len: u64) -> Option<Loaded> {
    let file = fs::read(path).ok()?;
    let snapshot: SnapshotRead = postcard::from_bytes(&file).ok()?;
    if snapshot.magic != MAGIC || snapshot.end != MAGIC || snapshot.log_len != log_len {
        return None;
    }
    let count = usize::try_from(snapshot.slots).ok()?;
    if snapshot.have.len() != count {
        return None;
    }
    let graph_at = (snapshot.graph.as_ptr() as usize).checked_sub(file.as_ptr() as usize)?;
    if graph_at.checked_add(snapshot.graph.len())? > file.len() {
        return None;
    }
    let (next_seq, dim) = (snapshot.next_seq, snapshot.dim as usize);
    let have: Vec<bool> = snapshot.have.iter().map(|&b| b != 0).collect();
    let (slots, bm25, ents, graph_ok) = std::thread::scope(|scope| {
        let snapshot = &snapshot;
        let bm25 = scope.spawn(move || postcard::from_bytes::<Bm25>(snapshot.bm25).ok());
        let graph = scope.spawn(move || Graph::check(count, snapshot.graph));
        let ents = scope.spawn(move || {
            let (records, cooc) = postcard::from_bytes::<(Vec<EntityRec>, Vec<(u32, u32, u32, i64)>)>(snapshot.entities).ok()?;
            let table = EntityTable::from_parts(records, cooc);
            table.is_consistent(count).then_some(table)
        });
        let per_job = snapshot.notes.len().div_ceil(16).max(1);
        let jobs: Vec<_> = snapshot
            .notes
            .chunks(per_job)
            .map(|blocks| {
                scope.spawn(move || {
                    let mut notes: Vec<Option<Note>> = Vec::new();
                    for block in blocks {
                        notes.extend(postcard::from_bytes::<Vec<Option<Note>>>(block).ok()?);
                    }
                    Some(notes)
                })
            })
            .collect();
        let mut slots = Some(Vec::<Option<Note>>::with_capacity(count));
        for job in jobs {
            match job.join() {
                Ok(Some(part)) => {
                    if let Some(all) = slots.as_mut() {
                        all.extend(part);
                    }
                }
                _ => slots = None,
            }
        }
        (slots, bm25.join().ok().flatten(), ents.join().ok().flatten(), graph.join().unwrap_or(false))
    });
    drop(snapshot);
    let (slots, bm25, ents) = (slots?, bm25?, ents?);
    if slots.len() != count || bm25.len() != count || !graph_ok {
        return None;
    }
    Some(Loaded { next_seq, dim, slots, have, bm25, ents, graph: Graph::from_checked(count, file, graph_at) })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::TempDir;

    fn unit(dim: usize, hot: usize) -> Vec<f32> {
        let mut v = vec![0.0; dim];
        v[hot] = 1.0;
        v
    }

    #[test]
    fn dot_matches_the_plain_sum_for_odd_lengths() {
        let a: Vec<f32> = (0..19).map(|n| n as f32 * 0.5).collect();
        let b: Vec<f32> = (0..19).map(|n| 1.0 + n as f32).collect();
        let expected: f32 = a.iter().zip(&b).map(|(x, y)| x * y).sum();
        assert!((dot(&a, &b) - expected).abs() < 1e-3);
    }

    #[test]
    fn set_row_scan_and_clear() {
        let mut store = VecStore::empty(4, 3);
        store.set(0, &unit(4, 0));
        store.set(2, &unit(4, 2));
        assert!(store.row(1).is_none());
        assert_eq!(store.row(2).unwrap(), &unit(4, 2)[..]);
        let mut seen = Vec::new();
        store.scan(&unit(4, 2), |slot, score| seen.push((slot, score)));
        assert_eq!(seen, vec![(0, 0.0), (2, 1.0)]);
        store.clear(2);
        assert!(store.row(2).is_none());
        store.set(5, &unit(4, 1));
        assert_eq!(store.have.len(), 6);
        store.set(0, &[1.0]);
        assert_eq!(store.row(0).unwrap(), &unit(4, 0)[..]);
    }

    #[test]
    fn written_matrix_is_mapped_again_and_can_still_grow_and_change() {
        let dir = TempDir::new("vecstore");
        let path = dir.path().join("v.bin");
        let mut store = VecStore::empty(4, 2);
        store.set(0, &unit(4, 0));
        store.set(1, &unit(4, 1));
        store.write(&path).unwrap();
        assert!(store.mapped.is_some() && store.tail.is_empty());
        store.set(2, &unit(4, 2));
        store.set(0, &unit(4, 3));
        assert!(store.mapped.is_none());
        assert_eq!(store.row(0).unwrap(), &unit(4, 3)[..]);
        assert_eq!(store.row(1).unwrap(), &unit(4, 1)[..]);
        store.write(&path).unwrap();
        let again = VecStore::open(&path, 4, store.have.clone()).unwrap();
        assert_eq!(again.row(2).unwrap(), &unit(4, 2)[..]);
        assert_eq!(again.row(0).unwrap(), &unit(4, 3)[..]);
        assert!(VecStore::open(&path, 5, store.have.clone()).is_err());
        assert!(VecStore::open(&path, 4, vec![true; 2]).is_err());
    }

    #[test]
    fn a_store_without_vectors_removes_its_file() {
        let dir = TempDir::new("vecstore-empty");
        let path = dir.path().join("v.bin");
        let mut store = VecStore::empty(4, 1);
        store.set(0, &unit(4, 0));
        store.write(&path).unwrap();
        assert!(path.exists());
        store.clear(0);
        store.write(&path).unwrap();
        assert!(!path.exists());
        store.write(&path).unwrap();
    }

    #[test]
    fn bm25_documents_are_text_context_tags_and_entities() {
        let note = Note {
            id: "n000001".into(),
            text: "reads the file".into(),
            context: "loader".into(),
            tags: vec!["project-x".into()],
            entities: vec!["parse_config".into()],
            ..Note::default()
        };
        let tokens = note_tokens(&note);
        for expected in ["reads", "loader", "project", "x", "parse_config", "parse", "config"] {
            assert!(tokens.iter().any(|t| t == expected), "{expected} missing from {tokens:?}");
        }
        let index = build_bm25(&[Some(note), None]);
        assert_eq!(index.len(), 2);
        let hits = index.search(&["config".to_string()], 5);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].0, 0);
    }

    #[test]
    fn a_snapshot_round_trips_and_checks_its_stamp() {
        let dir = TempDir::new("snapshot");
        let path = dir.path().join(INDEX_FILE);
        let note = |n: usize| Note { id: format!("n{n:06}"), text: format!("hello {n}"), entities: vec!["alpha_one".into()], proof_count: 1, ..Note::default() };
        let mut slots: Vec<Option<Note>> = (0..1300).map(|n| Some(note(n))).collect();
        slots[7] = None;
        let have: Vec<bool> = (0..slots.len()).map(|n| n % 3 == 0).collect();
        let bm25 = build_bm25(&slots);
        let mut ents = EntityTable::default();
        let id = ents.create("alpha_one", 5);
        ents.attach(&[id], 1, 5, true);
        let mut graph = Graph::default();
        graph.add(3, 0, crate::graph::LinkKind::Semantic, 0.75);
        graph.add(1299, 12, crate::graph::LinkKind::Temporal, 0.5);
        let parts = Parts { log_len: 123, next_seq: 3, dim: 4, slots: &slots, have: &have, bm25: &bm25, ents: &ents, graph: &graph };
        save_snapshot(&path, &parts).unwrap();
        let back = load_snapshot(&path, 123).unwrap();
        assert_eq!(back.slots, slots);
        assert_eq!((back.next_seq, back.dim), (3, 4));
        assert_eq!(back.have, have);
        assert_eq!(back.bm25.len(), 1300);
        assert_eq!(back.ents.recs.len(), 1);
        assert_eq!(back.graph.count(), 2);
        assert!(load_snapshot(&path, 124).is_none(), "made for another log");
        let mut bytes = fs::read(&path).unwrap();
        bytes.truncate(bytes.len() - 5);
        fs::write(&path, &bytes).unwrap();
        assert!(load_snapshot(&path, 123).is_none(), "cut short");
        fs::write(&path, b"garbage").unwrap();
        assert!(load_snapshot(&path, 123).is_none());
        assert!(load_snapshot(&dir.path().join("missing.bin"), 123).is_none());
    }
}
