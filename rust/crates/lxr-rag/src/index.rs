//! The searchable form of a set of chunks, on disk and in memory, and the search over it.

use crate::chunk::{Chunk, ChunkKind};
use crate::rank;
use lxr_core::{hash64, load, save};
use lxr_text::{code_tokens, rrf, Bm25, Embedder, Vectors};
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use std::borrow::Cow;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

/// Version of the files an index is written as. 2 added the size and hash of the vectors
/// file, the embedder's probe vector and the table's own hash; an index of version 1 is
/// refused and has to be built again.
const FORMAT: u32 = 2;
const TABLE_FILE: &str = "chunks.bin";
const BM25_FILE: &str = "bm25.bin";
const VECTORS_FILE: &str = "vectors.bin";
/// Bytes hashed at a time when a file or a table is checked.
const DIGEST_BLOCK: usize = 1 << 16;
/// A text whose vector the table keeps. An embedder that does not give the same vector for
/// it is not the one that made the index's vectors, whatever the length of its vectors.
const PROBE: &str = "parse the config file and render a template, then close the socket";
/// Largest difference between two numbers of a probe vector for the embedders to be the same.
const PROBE_TOLERANCE: f32 = 1e-3;
/// Chunks tokenised, and so held as token lists, at one time.
const TOKEN_BATCH: usize = 4096;
/// Chunk texts embedded in one call.
const EMBED_BATCH: usize = 2048;

const KIND_DEFINITION: u8 = 0;
const KIND_PART: u8 = 1;
const KIND_MODULE: u8 = 2;

/// Which retrievers a search uses.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mode {
    /// Words and vectors fused; words alone when the index has no vectors or no embedder is given.
    Hybrid,
    /// BM25 over code-aware tokens alone.
    Words,
    /// Vector similarity alone. Finds nothing without vectors and an embedder.
    Meaning,
}

/// How many hits to return, with which retrievers, and the weight of the vectors.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct SearchOpts {
    /// Most hits to return.
    pub limit: usize,
    /// Which retrievers to use.
    pub mode: Mode,
    /// Weight of the vectors in [`Mode::Hybrid`], from 0 to 1. `None` is 0.3 for a query
    /// that looks like a symbol name and 0.5 for any other.
    pub alpha: Option<f32>,
}

impl Default for SearchOpts {
    fn default() -> SearchOpts {
        SearchOpts { limit: 5, mode: Mode::Hybrid, alpha: None }
    }
}

/// One search result.
#[derive(Clone, PartialEq, Debug)]
pub struct Hit {
    /// Path of the file from the repository root.
    pub path: String,
    /// First line of the definition the hit lies in, or of the chunk when it lies in none.
    pub start: u32,
    /// Last line of that definition or chunk.
    pub end: u32,
    /// Qualified name of the definition, empty when the hit lies in none.
    pub symbol: String,
    /// Declaration line of the definition, empty when the hit lies in none.
    pub sig: String,
    /// First sentence of the definition's documentation, empty when there is none.
    pub doc: String,
    /// The final combined score.
    pub score: f32,
    /// First line of the best chunk of the hit.
    pub chunk_start: u32,
    /// Last line of the best chunk of the hit.
    pub chunk_end: u32,
}

/// One chunk without its text. Strings are numbers into [`Table::strings`].
#[derive(Serialize, Deserialize, Clone, Debug)]
struct Row {
    path: u32,
    start: u32,
    end: u32,
    kind: u8,
    symbol: u32,
    sig: u32,
    doc: u32,
    def_start: u32,
    def_end: u32,
}

/// The names a module chunk defines, found in its text when the index was built.
#[derive(Serialize, Deserialize, Clone, Debug)]
struct ModuleNames {
    chunk: u32,
    /// Names after a definition word, as written.
    general: Vec<String>,
    /// Names after a SQL definition, lowercased.
    sql: Vec<String>,
}

/// The chunk table: everything about the chunks except their text and their words.
#[derive(Serialize, Deserialize, Debug)]
struct Table {
    format: u32,
    paths: Vec<String>,
    /// Qualified names, declarations and documentation lines; entry 0 is the empty string.
    strings: Vec<String>,
    rows: Vec<Row>,
    modules: Vec<ModuleNames>,
    /// Length of the vectors, 0 when the index has none.
    dim: u32,
    /// Size and hash of the BM25 file, to notice a damaged copy or one from another build.
    bm25_len: u64,
    bm25_hash: u64,
    /// The same for the vectors file; 0 and 0 when the index has none.
    vectors_len: u64,
    vectors_hash: u64,
    /// The vector the embedder gave for [`PROBE`] when the index was built; empty when the
    /// index has no vectors.
    probe: Vec<f32>,
    /// Hash of everything above (see [`Table::digest`]), to notice a damaged table.
    check: u64,
}

impl Table {
    /// A hash of every field but `check`, taken from the values and not from the bytes the
    /// table was stored as, so it holds for what was read, however it was encoded.
    fn digest(&self) -> u64 {
        let mut digest = Digest::new();
        digest.number(u64::from(self.format));
        digest.number(self.paths.len() as u64);
        for path in &self.paths {
            digest.text(path);
        }
        digest.number(self.strings.len() as u64);
        for string in &self.strings {
            digest.text(string);
        }
        digest.number(self.rows.len() as u64);
        for r in &self.rows {
            for n in [r.path, r.start, r.end, u32::from(r.kind), r.symbol, r.sig, r.doc, r.def_start, r.def_end] {
                digest.number(u64::from(n));
            }
        }
        digest.number(self.modules.len() as u64);
        for m in &self.modules {
            digest.number(u64::from(m.chunk));
            digest.number(m.general.len() as u64);
            for name in &m.general {
                digest.text(name);
            }
            digest.number(m.sql.len() as u64);
            for name in &m.sql {
                digest.text(name);
            }
        }
        for n in [u64::from(self.dim), self.bm25_len, self.bm25_hash, self.vectors_len, self.vectors_hash, self.probe.len() as u64] {
            digest.number(n);
        }
        for x in &self.probe {
            digest.number(u64::from(x.to_bits()));
        }
        digest.finish()
    }
}

/// A hash of a stream of bytes that does not depend on how the stream is cut into the
/// pieces handed to [`Digest::put`]: bytes are gathered into blocks of [`DIGEST_BLOCK`],
/// and each full block is hashed together with the hash of the blocks before it.
struct Digest {
    state: u64,
    block: Vec<u8>,
}

impl Digest {
    fn new() -> Digest {
        Digest { state: 0, block: Vec::with_capacity(DIGEST_BLOCK + 8) }
    }

    fn put(&mut self, mut bytes: &[u8]) {
        while !bytes.is_empty() {
            let take = (DIGEST_BLOCK - self.block.len()).min(bytes.len());
            self.block.extend_from_slice(&bytes[..take]);
            bytes = &bytes[take..];
            if self.block.len() == DIGEST_BLOCK {
                self.fold();
            }
        }
    }

    fn number(&mut self, n: u64) {
        self.put(&n.to_le_bytes());
    }

    /// A string with its length in front, so that two strings never run into each other.
    fn text(&mut self, s: &str) {
        self.number(s.len() as u64);
        self.put(s.as_bytes());
    }

    fn fold(&mut self) {
        self.block.extend_from_slice(&self.state.to_le_bytes());
        self.state = hash64(&self.block);
        self.block.clear();
    }

    fn finish(mut self) -> u64 {
        self.fold();
        self.state
    }
}

/// Size and hash of the file at `path`, read in blocks so that a large file is never held whole.
fn file_digest(path: &Path) -> io::Result<(u64, u64)> {
    let mut file = fs::File::open(path)?;
    let mut digest = Digest::new();
    let mut size = 0u64;
    let mut buffer = vec![0u8; DIGEST_BLOCK];
    loop {
        match file.read(&mut buffer) {
            Ok(0) => break,
            Ok(n) => {
                digest.put(&buffer[..n]);
                size += n as u64;
            }
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(error) => return Err(error),
        }
    }
    Ok((size, digest.finish()))
}

/// Errors unless the file at `path` has the size and hash that the table recorded for it.
fn verify(path: &Path, size: u64, hash: u64) -> io::Result<()> {
    if file_digest(path)? != (size, hash) {
        return Err(invalid(format!("{} is damaged", path.display())));
    }
    Ok(())
}

/// Has `write` write a file under a temporary name beside `path`, takes the size and hash of
/// that file and moves it to `path`. The figures describe the bytes that end up at `path`,
/// not whatever another writer may have put there meanwhile.
fn write_checked(path: &Path, write: impl FnOnce(&Path) -> io::Result<()>) -> io::Result<(u64, u64)> {
    // The process id keeps two processes apart, the counter two threads of one process.
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let mut name = path.as_os_str().to_owned();
    name.push(format!(".{}.{}.check", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
    let temp = PathBuf::from(name);
    let done = write(&temp).and_then(|()| file_digest(&temp)).and_then(|figures| fs::rename(&temp, path).map(|()| figures));
    if done.is_err() {
        let _ = fs::remove_file(&temp);
    }
    done
}

/// What is known about a file from its path alone.
#[derive(Debug)]
struct FileInfo {
    /// File stem, lowercased.
    stem: String,
    /// The stem without underscores.
    stem_joined: String,
    /// Words of the stem and the folder name.
    parts: HashSet<String>,
    /// Factor for the scores of the file's chunks.
    penalty: f64,
}

fn file_info(path: &str) -> FileInfo {
    let stem = rank::file_stem(path).to_lowercase();
    FileInfo { stem_joined: stem.replace('_', ""), stem, parts: rank::path_parts(path), penalty: rank::path_penalty(path) }
}

/// Hands out numbers for strings, the same number for the same string.
#[derive(Default)]
struct Interner {
    ids: HashMap<String, u32>,
    list: Vec<String>,
}

impl Interner {
    fn id(&mut self, s: &str) -> u32 {
        if let Some(&id) = self.ids.get(s) {
            return id;
        }
        let id = self.list.len() as u32;
        self.ids.insert(s.to_string(), id);
        self.list.push(s.to_string());
        id
    }
}

/// The searchable form of a set of chunks: a chunk table, a BM25 index over their words
/// and, when an embedder was given at build time, one vector per chunk.
#[derive(Debug)]
pub struct SearchIndex {
    table: Table,
    files: Vec<FileInfo>,
    /// Numbers of the chunks of each file, in order.
    file_chunks: Vec<Vec<u32>>,
    modules: HashMap<u32, ModuleNames>,
    bm25: Bm25,
    vectors: Option<Vectors>,
}

fn invalid(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.into())
}

/// The text of a chunk as it is indexed, for words and for vectors: its own text, with the
/// documentation line of its definition in front when the chunk is the one that begins the
/// definition and does not hold that line already. The documentation of a Rust, Java or
/// JavaScript definition is a comment above the declaration, outside the definition's lines,
/// so it sits in a module chunk; this lets a search for it find the definition as well.
/// A Python docstring is in the chunk already, and then nothing is added.
fn indexed_text(chunk: &Chunk) -> Cow<'_, str> {
    // A documentation line cut to 200 characters ends in "...".
    let doc = chunk.doc.strip_suffix("...").unwrap_or(&chunk.doc);
    let begins = chunk.kind != ChunkKind::Module && chunk.start == chunk.def_start;
    if !begins || doc.is_empty() || chunk.text.split_whitespace().collect::<Vec<_>>().join(" ").contains(doc) {
        return Cow::Borrowed(&chunk.text);
    }
    Cow::Owned(format!("{doc}\n{}", chunk.text))
}

/// The text a chunk is indexed for words as: its indexed text, the file stem twice, the
/// names of the last three folders and the qualified name of its definition.
fn words_text(chunk: &Chunk) -> String {
    let stem = rank::file_stem(&chunk.path);
    let folders = rank::folder_names(&chunk.path);
    let near = folders[folders.len().saturating_sub(3)..].join(" ");
    format!("{} {stem} {stem} {near} {}", indexed_text(chunk), chunk.symbol)
}

/// Checks the table on its own: every row points at strings and a path that exist, has its
/// lines in order and a known kind; the string table starts with the empty string; and a
/// module entry points at a module chunk.
fn check_table(table: &Table) -> io::Result<()> {
    let (paths, strings) = (table.paths.len() as u32, table.strings.len() as u32);
    for (i, row) in table.rows.iter().enumerate() {
        let sound = row.path < paths
            && row.symbol < strings
            && row.sig < strings
            && row.doc < strings
            && row.kind <= KIND_MODULE
            && row.start <= row.end
            && row.def_start <= row.def_end;
        if !sound {
            return Err(invalid(format!("chunk {i} of the table is not valid")));
        }
    }
    if table.strings.first().map(String::as_str) != Some("") {
        return Err(invalid("the string table does not start with the empty string"));
    }
    for m in &table.modules {
        if m.chunk as usize >= table.rows.len() || table.rows[m.chunk as usize].kind != KIND_MODULE {
            return Err(invalid("a module entry does not point at a module chunk"));
        }
    }
    Ok(())
}

impl SearchIndex {
    /// Builds the index and writes its files into `dir`, which is created.
    ///
    /// `chunks.bin` holds the chunk table (no chunk text), `bm25.bin` the word index and,
    /// when `embedder` is given, `vectors.bin` one vector of each chunk's text. Chunks are
    /// ordered by path and first line, which is also the order that equal scores keep.
    ///
    /// The table is checked before anything is written, so chunks that cannot make a sound
    /// table give an error and leave a folder that held an index as it was. The table is
    /// written last and records the size and hash of the other two files, so a build that
    /// stops half way leaves a folder that [`SearchIndex::open`] refuses.
    pub fn build(mut chunks: Vec<Chunk>, embedder: Option<&Embedder>, dir: &Path) -> io::Result<SearchIndex> {
        if chunks.len() >= u32::MAX as usize {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "too many chunks: numbers are 32-bit"));
        }
        chunks.sort_by(|a, b| (a.path.as_str(), a.start, a.end).cmp(&(b.path.as_str(), b.start, b.end)));

        let mut paths = Interner::default();
        let mut strings = Interner::default();
        strings.id("");
        let rows: Vec<Row> = chunks
            .iter()
            .map(|c| Row {
                path: paths.id(&c.path),
                start: c.start,
                end: c.end,
                kind: match c.kind {
                    ChunkKind::Definition => KIND_DEFINITION,
                    ChunkKind::Part => KIND_PART,
                    ChunkKind::Module => KIND_MODULE,
                },
                symbol: strings.id(&c.symbol),
                sig: strings.id(&c.sig),
                doc: strings.id(&c.doc),
                def_start: c.def_start,
                def_end: c.def_end,
            })
            .collect();
        let modules: Vec<ModuleNames> = chunks
            .par_iter()
            .enumerate()
            .filter(|(_, c)| c.kind == ChunkKind::Module)
            .filter_map(|(i, c)| {
                let (general, sql) = rank::module_definitions(&c.text);
                (!general.is_empty() || !sql.is_empty()).then(|| ModuleNames { chunk: i as u32, general, sql })
            })
            .collect();
        let mut table = Table {
            format: FORMAT,
            paths: paths.list,
            strings: strings.list,
            rows,
            modules,
            dim: 0,
            bm25_len: 0,
            bm25_hash: 0,
            vectors_len: 0,
            vectors_hash: 0,
            probe: Vec::new(),
            check: 0,
        };
        check_table(&table)?;
        fs::create_dir_all(dir)?;

        let total = chunks.len();
        let docs = (0..total).step_by(TOKEN_BATCH).flat_map(|from| {
            let to = (from + TOKEN_BATCH).min(total);
            chunks[from..to].par_iter().map(|c| code_tokens(&words_text(c))).collect::<Vec<_>>()
        });
        let bm25 = Bm25::build(docs);

        let vectors_path = dir.join(VECTORS_FILE);
        match embedder {
            Some(e) if total > 0 => {
                let mut matrix: Vec<Vec<f32>> = Vec::with_capacity(total);
                for batch in chunks.chunks(EMBED_BATCH) {
                    let texts: Vec<Cow<str>> = batch.iter().map(indexed_text).collect();
                    let views: Vec<&str> = texts.iter().map(|t| t.as_ref()).collect();
                    matrix.extend(e.embed_many(&views));
                }
                let (size, hash) = write_checked(&vectors_path, |temp| Vectors::write(temp, e.dim(), &matrix))?;
                table.dim = e.dim() as u32;
                table.vectors_len = size;
                table.vectors_hash = hash;
                table.probe = e.embed(PROBE);
            }
            _ => {
                let _ = fs::remove_file(&vectors_path);
            }
        }
        drop(chunks);

        let (size, hash) = write_checked(&dir.join(BM25_FILE), |temp| save(temp, &bm25))?;
        table.bm25_len = size;
        table.bm25_hash = hash;
        table.check = table.digest();
        save(&dir.join(TABLE_FILE), &table)?;
        let vectors = if table.dim > 0 { Some(Vectors::open(&vectors_path)?) } else { None };
        SearchIndex::from_parts(table, bm25, vectors)
    }

    /// Opens an index written by [`SearchIndex::build`]. A missing folder or file, a file
    /// of another version, a damaged file, or files that do not belong together give an error.
    /// The table is checked against its own hash, and the word index and the vectors against
    /// the size and hash that the table recorded for them.
    pub fn open(dir: &Path) -> io::Result<SearchIndex> {
        let table_path = dir.join(TABLE_FILE);
        let table: Table = load(&table_path)?;
        if table.format != FORMAT {
            return Err(invalid(format!("index format {} is not {FORMAT}", table.format)));
        }
        if table.check != table.digest() {
            return Err(invalid(format!("{} is damaged", table_path.display())));
        }
        // A file is read into its structure first and checked after: a file that a rebuild
        // replaced in between fails the check instead of being taken on the strength of
        // a check made on the file it replaced.
        let bm25_path = dir.join(BM25_FILE);
        let bm25: io::Result<Bm25> = load(&bm25_path);
        verify(&bm25_path, table.bm25_len, table.bm25_hash)?;
        let vectors = if table.dim > 0 {
            let vectors_path = dir.join(VECTORS_FILE);
            let vectors = Vectors::open(&vectors_path);
            verify(&vectors_path, table.vectors_len, table.vectors_hash)?;
            Some(vectors?)
        } else {
            None
        };
        SearchIndex::from_parts(table, bm25?, vectors)
    }

    /// Checks that the parts agree with each other and derives what search needs.
    fn from_parts(table: Table, bm25: Bm25, vectors: Option<Vectors>) -> io::Result<SearchIndex> {
        let n = table.rows.len();
        if table.format != FORMAT || bm25.len() != n {
            return Err(invalid("the word index does not match the chunk table"));
        }
        match &vectors {
            Some(v) if v.len() != n || v.dim() != table.dim as usize => {
                return Err(invalid("the vectors do not match the chunk table"));
            }
            None if table.dim != 0 => return Err(invalid("the vectors are missing")),
            _ => {}
        }
        if table.probe.len() != table.dim as usize {
            return Err(invalid("the embedder's probe vector does not match the vectors"));
        }
        check_table(&table)?;
        let mut file_chunks: Vec<Vec<u32>> = vec![Vec::new(); table.paths.len()];
        for (i, row) in table.rows.iter().enumerate() {
            file_chunks[row.path as usize].push(i as u32);
        }
        let modules: HashMap<u32, ModuleNames> = table.modules.iter().map(|m| (m.chunk, m.clone())).collect();
        let files = table.paths.iter().map(|p| file_info(p)).collect();
        Ok(SearchIndex { table, files, file_chunks, modules, bm25, vectors })
    }

    /// Number of chunks.
    pub fn len(&self) -> usize {
        self.table.rows.len()
    }

    /// True when the index holds no chunk.
    pub fn is_empty(&self) -> bool {
        self.table.rows.is_empty()
    }

    /// True when the index has a vector for every chunk.
    pub fn has_vectors(&self) -> bool {
        self.vectors.is_some()
    }

    /// The best hits for `query`, best first, at most `opts.limit`.
    ///
    /// Each retriever offers five times `limit` chunks, and twice as many again, repeatedly,
    /// while that gives fewer than `limit` hits and a retriever had more to offer (one
    /// long definition can fill the first pool with its parts). Their ranks are fused (reciprocal
    /// rank, constant 60, the vectors weighted by alpha), then the best chunk of the file
    /// with the most good chunks is lifted, a query that names a symbol lifts the chunks
    /// that define it and any other query lifts files whose names match its words, chunks
    /// of one definition are merged into one hit that keeps the best score, test and
    /// similar paths are pushed down, and each further hit from a file counts half as much
    /// as the one before. Ties are broken by path and line.
    pub fn search(&self, query: &str, embedder: Option<&Embedder>, opts: &SearchOpts) -> Vec<Hit> {
        let query = query.trim();
        if query.is_empty() || opts.limit == 0 || self.is_empty() {
            return Vec::new();
        }
        let meaning = if opts.mode == Mode::Words { None } else { self.query_vector(query, embedder) };
        let words = opts.mode != Mode::Meaning;
        if !words && meaning.is_none() {
            return Vec::new();
        }
        let alpha = match (words, meaning.is_some()) {
            (true, true) => opts.alpha.unwrap_or_else(|| rank::auto_alpha(query)).clamp(0.0, 1.0),
            (true, false) => 0.0,
            _ => 1.0,
        };
        let mut wanted = opts.limit.saturating_mul(5);
        loop {
            let (hits, more) = self.search_pool(query, meaning.as_deref(), alpha, opts.limit, wanted);
            if hits.len() >= opts.limit || !more || wanted >= self.len() {
                return hits;
            }
            wanted = wanted.saturating_mul(2);
        }
    }

    /// One pass of [`SearchIndex::search`] over the best `wanted` chunks of each retriever.
    /// The second value is true when a retriever had at least `wanted` chunks to offer, so
    /// that a larger `wanted` could bring in more.
    fn search_pool(&self, query: &str, meaning: Option<&[f32]>, alpha: f32, limit: usize, wanted: usize) -> (Vec<Hit>, bool) {
        let semantic: Vec<u32> = match (meaning, &self.vectors) {
            (Some(q), Some(v)) if alpha > 0.0 => v.nearest(q, wanted).into_iter().map(|h| h.0).collect(),
            _ => Vec::new(),
        };
        let lexical: Vec<u32> = if alpha < 1.0 {
            self.bm25.search(&code_tokens(query), wanted).into_iter().map(|h| h.0).collect()
        } else {
            Vec::new()
        };
        let more = semantic.len() >= wanted || lexical.len() >= wanted;
        let mut scores: BTreeMap<u32, f64> = rrf(&[semantic.as_slice(), lexical.as_slice()], rank::RRF_K, &[alpha, 1.0 - alpha])
            .into_iter()
            .filter(|&(_, score)| score != 0.0)
            .map(|(id, score)| (id, f64::from(score)))
            .collect();
        if scores.is_empty() {
            return (Vec::new(), more);
        }
        self.lift_files(&mut scores);
        self.lift_query(&mut scores, query);
        let merged = self.merge_definitions(&scores);
        let hits = self.rerank(merged, limit, alpha < 1.0).into_iter().map(|(id, score)| self.hit(id, score)).collect();
        (hits, more)
    }

    /// The unit vector of the query, when the index has vectors, an embedder of the same
    /// length and the same probe vector is given, and the query has a word the model knows.
    fn query_vector(&self, query: &str, embedder: Option<&Embedder>) -> Option<Vec<f32>> {
        let vectors = self.vectors.as_ref()?;
        let embedder = embedder.filter(|e| e.dim() == vectors.dim() && self.same_model(e))?;
        let vector = embedder.embed(query);
        vector.iter().any(|&x| x != 0.0).then_some(vector)
    }

    /// True when `embedder` gives for [`PROBE`] the vector that the embedder which made the
    /// index's vectors gave. Vectors of another model would rank without meaning.
    fn same_model(&self, embedder: &Embedder) -> bool {
        let probe = embedder.embed(PROBE);
        probe.len() == self.table.probe.len() && probe.iter().zip(&self.table.probe).all(|(a, b)| (a - b).abs() <= PROBE_TOLERANCE)
    }

    fn string(&self, id: u32) -> &str {
        &self.table.strings[id as usize]
    }

    fn file_of(&self, chunk: u32) -> &FileInfo {
        &self.files[self.table.rows[chunk as usize].path as usize]
    }

    /// Adds to the best chunk of each file a share of the best score, in proportion to
    /// how much of the candidates' score the file holds.
    fn lift_files(&self, scores: &mut BTreeMap<u32, f64>) {
        let max = scores.values().copied().fold(0.0, f64::max);
        if max == 0.0 {
            return;
        }
        let mut sum: HashMap<u32, f64> = HashMap::new();
        let mut best: HashMap<u32, (u32, f64)> = HashMap::new();
        for (&id, &score) in scores.iter() {
            let path = self.table.rows[id as usize].path;
            *sum.entry(path).or_insert(0.0) += score;
            match best.get(&path) {
                Some(&(_, kept)) if score <= kept => {}
                _ => {
                    best.insert(path, (id, score));
                }
            }
        }
        let max_sum = sum.values().copied().fold(0.0, f64::max);
        let unit = max * rank::FILE_BOOST;
        for (path, (id, _)) in best {
            if let Some(score) = scores.get_mut(&id) {
                *score += unit * sum[&path] / max_sum;
            }
        }
    }

    /// Applies the boosts of the query's kind.
    fn lift_query(&self, scores: &mut BTreeMap<u32, f64>, query: &str) {
        let max = scores.values().copied().fold(0.0, f64::max);
        if rank::is_symbol_query(query) {
            let name = rank::symbol_name(query).to_lowercase();
            let names = rank::symbol_names(query);
            self.lift_definitions(scores, &names, max * rank::DEFINITION_BOOST, &|f: &FileInfo| rank::stem_matches(&f.stem, &name));
        } else {
            self.lift_stems(scores, query, max);
            let names = rank::embedded_symbols(query);
            if !names.is_empty() {
                let lower: Vec<String> = names.iter().map(|n| n.to_lowercase()).collect();
                let unit = max * rank::DEFINITION_BOOST * rank::EMBEDDED_SCALE;
                self.lift_definitions(scores, &names, unit, &|f: &FileInfo| rank::embedded_stem_ok(&f.stem, &f.stem_joined, &lower));
            }
        }
    }

    /// Lifts candidates whose file name or folder matches the words of the query.
    fn lift_stems(&self, scores: &mut BTreeMap<u32, f64>, query: &str, max: f64) {
        let keywords = rank::keywords(query);
        if keywords.is_empty() {
            return;
        }
        let unit = max * rank::STEM_BOOST;
        for (&id, score) in scores.iter_mut() {
            let matches = rank::keyword_matches(&keywords, &self.file_of(id).parts);
            if matches > 0 && rank::enough_matches(matches, keywords.len()) {
                *score += unit * matches as f64 / keywords.len() as f64;
            }
        }
    }

    /// Lifts the chunks that define one of `names` by `unit`, half as much again when the
    /// file stem is the name too. Chunks that are not candidates yet enter with that lift
    /// alone when their file passes `stem_ok`.
    fn lift_definitions(&self, scores: &mut BTreeMap<u32, f64>, names: &[String], unit: f64, stem_ok: &dyn Fn(&FileInfo) -> bool) {
        let lower: Vec<String> = names.iter().map(|n| n.to_lowercase()).collect();
        let tier = |id: u32| -> f64 {
            if !self.defines(id, names, &lower) {
                return 0.0;
            }
            let stem = &self.file_of(id).stem;
            unit * if lower.iter().any(|n| rank::stem_matches(stem, n)) { 1.5 } else { 1.0 }
        };
        let candidates: Vec<u32> = scores.keys().copied().collect();
        for id in candidates {
            let lift = tier(id);
            if lift != 0.0 {
                if let Some(score) = scores.get_mut(&id) {
                    *score += lift;
                }
            }
        }
        for (path, file) in self.files.iter().enumerate() {
            if !stem_ok(file) {
                continue;
            }
            for &id in &self.file_chunks[path] {
                if scores.contains_key(&id) {
                    continue;
                }
                let lift = tier(id);
                if lift != 0.0 {
                    scores.insert(id, lift);
                }
            }
        }
    }

    /// True when the chunk is, or for a module chunk contains, the definition of one of the names.
    fn defines(&self, id: u32, names: &[String], lower: &[String]) -> bool {
        let row = &self.table.rows[id as usize];
        if row.kind == KIND_MODULE {
            return self.modules.get(&id).is_some_and(|m| {
                m.general.iter().any(|g| names.contains(g)) || m.sql.iter().any(|s| lower.contains(s))
            });
        }
        let qualified = self.string(row.symbol);
        if qualified.is_empty() {
            return false;
        }
        let bare = qualified.rsplit('.').next().unwrap_or(qualified);
        names.iter().any(|n| n == bare || n == qualified)
    }

    /// One entry for the chunks of one definition, with the best score of them. Module
    /// chunks stand for themselves. Entries come back in chunk order.
    fn merge_definitions(&self, scores: &BTreeMap<u32, f64>) -> Vec<(u32, f64)> {
        let mut best: HashMap<(u32, u32, u32, u32, bool), (u32, f64)> = HashMap::new();
        for (&id, &score) in scores {
            let row = &self.table.rows[id as usize];
            let key = if row.kind == KIND_MODULE {
                (row.path, row.start, row.end, 0, true)
            } else {
                (row.path, row.def_start, row.def_end, row.symbol, false)
            };
            match best.get(&key) {
                Some(&(_, kept)) if score <= kept => {}
                _ => {
                    best.insert(key, (id, score));
                }
            }
        }
        let mut merged: Vec<(u32, f64)> = best.into_values().collect();
        merged.sort_by_key(|&(id, _)| id);
        merged
    }

    /// Picks the best `limit` entries. Scores of paths that are tests, compatibility code,
    /// examples and the like are scaled down first when `penalise` is set; then entries are
    /// taken best first, each further one from a file counting half as much as the one
    /// before it from that file, until the next entry cannot beat the worst one taken.
    fn rerank(&self, merged: Vec<(u32, f64)>, limit: usize, penalise: bool) -> Vec<(u32, f64)> {
        let mut ranked: Vec<(u32, f64)> = merged
            .into_iter()
            .map(|(id, score)| (id, if penalise { score * self.file_of(id).penalty } else { score }))
            .collect();
        ranked.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));
        let mut per_file: HashMap<u32, i32> = HashMap::new();
        let mut taken: Vec<(f64, u32)> = Vec::new();
        let mut worst = f64::INFINITY;
        for (id, score) in ranked {
            if taken.len() >= limit && score <= worst {
                break;
            }
            let path = self.table.rows[id as usize].path;
            let before = per_file.entry(path).or_insert(0);
            let effective = if *before >= 1 { score * 0.5f64.powi(*before) } else { score };
            *before += 1;
            taken.push((effective, id));
            if taken.len() >= limit {
                worst = taken.iter().map(|t| t.0).fold(f64::INFINITY, f64::min);
            }
        }
        taken.sort_by(|a, b| b.0.total_cmp(&a.0));
        taken.truncate(limit);
        taken.into_iter().map(|(score, id)| (id, score)).collect()
    }

    fn hit(&self, id: u32, score: f64) -> Hit {
        let row = &self.table.rows[id as usize];
        let module = row.kind == KIND_MODULE;
        Hit {
            path: self.table.paths[row.path as usize].clone(),
            start: if module { row.start } else { row.def_start },
            end: if module { row.end } else { row.def_end },
            symbol: self.string(row.symbol).to_string(),
            sig: self.string(row.sig).to_string(),
            doc: self.string(row.doc).to_string(),
            score: score as f32,
            chunk_start: row.start,
            chunk_end: row.end,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("lxr-rag-{}-{}", name, std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// A model with three axes. "parse", "config" and "file" point along the first, "render",
    /// "html" and "template" along the second, "socket", "connect" and "retry" along the
    /// third. Every other word is unknown.
    fn model(dir: &Path) -> Embedder {
        let (a, b, c): (&[f32], &[f32], &[f32]) = (&[1.0, 0.0, 0.0], &[0.0, 1.0, 0.0], &[0.0, 0.0, 1.0]);
        let words = [("parse", a), ("config", a), ("file", a), ("render", b), ("html", b), ("template", b)];
        let rest = [("socket", c), ("connect", c), ("retry", c)];
        model_of(dir, 3, &[words.as_slice(), rest.as_slice()].concat())
    }

    /// A model whose vectors have `dim` numbers: each word given has the vector beside it,
    /// every other word is unknown.
    fn model_of(dir: &Path, dim: usize, words: &[(&str, &[f32])]) -> Embedder {
        fs::create_dir_all(dir).unwrap();
        let mut vocab = String::from(r#""[UNK]":0"#);
        for (i, (word, _)) in words.iter().enumerate() {
            vocab.push_str(&format!(r#","{word}":{}"#, i + 1));
        }
        let pad = words.len() + 1;
        vocab.push_str(&format!(r#","[PAD]":{pad}"#));
        let tokenizer = format!(
            r###"{{"truncation":{{"direction":"Right","max_length":512,"strategy":"LongestFirst","stride":0}},"padding":{{"strategy":"BatchLongest","direction":"Right","pad_to_multiple_of":null,"pad_id":{pad},"pad_type_id":0,"pad_token":"[PAD]"}},"added_tokens":[{{"id":0,"content":"[UNK]","single_word":false,"lstrip":false,"rstrip":false,"normalized":false,"special":true}},{{"id":{pad},"content":"[PAD]","single_word":true,"lstrip":true,"rstrip":true,"normalized":true,"special":true}}],"normalizer":{{"type":"BertNormalizer","clean_text":true,"handle_chinese_chars":true,"strip_accents":null,"lowercase":true}},"pre_tokenizer":{{"type":"BertPreTokenizer"}},"model":{{"type":"WordPiece","unk_token":"[UNK]","continuing_subword_prefix":"##","max_input_chars_per_word":100,"vocab":{{{vocab}}}}}}}"###
        );
        fs::write(dir.join("tokenizer.json"), tokenizer).unwrap();
        let zero = vec![0.0f32; dim];
        let mut data: Vec<u8> = Vec::new();
        data.extend(zero.iter().flat_map(|v| v.to_le_bytes()));
        for (_, vector) in words {
            assert_eq!(vector.len(), dim);
            data.extend(vector.iter().flat_map(|v| v.to_le_bytes()));
        }
        data.extend(zero.iter().flat_map(|v| v.to_le_bytes()));
        let header = format!(r#"{{"embeddings":{{"dtype":"F32","shape":[{},{dim}],"data_offsets":[0,{}]}}}}"#, words.len() + 2, data.len());
        let mut file = (header.len() as u64).to_le_bytes().to_vec();
        file.extend_from_slice(header.as_bytes());
        file.extend_from_slice(&data);
        fs::write(dir.join("model.safetensors"), file).unwrap();
        Embedder::open_with_cache(dir, &dir.join("cache")).unwrap()
    }

    #[allow(clippy::too_many_arguments)]
    fn chunk(path: &str, start: u32, end: u32, kind: ChunkKind, symbol: &str, def: (u32, u32), text: &str) -> Chunk {
        Chunk {
            path: path.to_string(),
            start,
            end,
            kind,
            symbol: symbol.to_string(),
            sig: if symbol.is_empty() { String::new() } else { format!("def {symbol}()") },
            doc: if symbol.is_empty() { String::new() } else { format!("About {symbol}.") },
            def_start: def.0,
            def_end: def.1,
            text: text.to_string(),
        }
    }

    /// Ten chunks over six files. Chunk order after the build is by path and line.
    fn corpus() -> Vec<Chunk> {
        use ChunkKind::*;
        vec![
            chunk("src/config.py", 1, 4, Definition, "load_config", (1, 4), "def load_config(path):\n    text = read_file(path)\n    return parse(text)"),
            chunk("src/config.py", 6, 8, Definition, "dump_config", (6, 8), "def dump_config(data):\n    return render(data)"),
            chunk("src/render.py", 1, 3, Definition, "render_page", (1, 3), "def render_page(html):\n    return template(html)"),
            chunk("src/render.py", 5, 7, Definition, "escape", (5, 7), "def escape(text):\n    return template(text)"),
            chunk("src/server.py", 1, 5, Part, "Server", (1, 12), "class Server:\n    def connect(self, socket):\n        retry(socket)"),
            chunk("src/server.py", 6, 12, Part, "Server", (1, 12), "        retry the socket connect again\n        retry connect"),
            chunk("src/util.py", 1, 2, Module, "", (0, 0), "import os\nimport sys"),
            chunk("src/util.py", 4, 6, Definition, "helper", (4, 6), "def helper():\n    return config_file_parse"),
            chunk("tests/test_config.py", 1, 3, Definition, "test_load_config", (1, 3), "def test_load_config():\n    load_config(path)\n    parse config file text"),
            chunk("src/__init__.py", 1, 2, Module, "", (0, 0), "from .config import load_config\nclass Legacy"),
        ]
    }

    fn built(name: &str) -> (SearchIndex, Embedder, PathBuf) {
        let dir = temp(name);
        let embedder = model(&dir.join("model"));
        let index = SearchIndex::build(corpus(), Some(&embedder), &dir.join("index")).unwrap();
        (index, embedder, dir)
    }

    fn opts(limit: usize, mode: Mode) -> SearchOpts {
        SearchOpts { limit, mode, alpha: None }
    }

    fn names(hits: &[Hit]) -> Vec<String> {
        hits.iter().map(|h| format!("{}:{}", h.path, h.symbol)).collect()
    }

    #[test]
    fn the_corpus_gives_these_exact_orders() {
        let (index, embedder, _dir) = built("orders");
        // (query, mode, limit, hits as "path:symbol"). A module chunk has no symbol.
        let cases: Vec<(&str, Mode, usize, Vec<&str>)> = vec![
            // A symbol query: the definition first, then what the words and vectors fuse to.
            (
                "load_config",
                Mode::Hybrid,
                5,
                vec!["src/config.py:load_config", "src/util.py:helper", "src/__init__.py:", "src/config.py:dump_config", "src/render.py:render_page"],
            ),
            (
                "load_config",
                Mode::Words,
                5,
                vec![
                    "src/config.py:load_config",
                    "src/util.py:helper",
                    "src/__init__.py:",
                    "src/config.py:dump_config",
                    "tests/test_config.py:test_load_config",
                ],
            ),
            ("load_config", Mode::Meaning, 3, vec!["src/config.py:load_config", "src/util.py:helper", "src/render.py:render_page"]),
            // Natural language.
            (
                "render the html template",
                Mode::Hybrid,
                4,
                vec!["src/render.py:render_page", "src/config.py:dump_config", "src/server.py:Server", "src/render.py:escape"],
            ),
            (
                "render the html template",
                Mode::Words,
                4,
                vec!["src/render.py:render_page", "src/server.py:Server", "src/config.py:dump_config", "src/render.py:escape"],
            ),
            ("render the html template", Mode::Meaning, 3, vec!["src/render.py:render_page", "src/config.py:dump_config", "src/server.py:Server"]),
            ("retry connect socket", Mode::Words, 5, vec!["src/server.py:Server"]),
            ("config", Mode::Words, 3, vec!["src/config.py:dump_config", "src/config.py:load_config", "src/util.py:helper"]),
            // These queries have no word the model knows: Hybrid is Words, Meaning has nothing to compare.
            ("Server", Mode::Hybrid, 5, vec!["src/server.py:Server"]),
            ("Server", Mode::Meaning, 5, vec![]),
            ("how to dump data", Mode::Hybrid, 5, vec!["src/config.py:dump_config"]),
        ];
        for (query, mode, limit, expected) in cases {
            let hits = index.search(query, Some(&embedder), &opts(limit, mode));
            assert_eq!(names(&hits), expected, "{query:?} {mode:?}");
        }
    }

    #[test]
    fn model_vectors_are_what_the_tests_assume() {
        let dir = temp("model");
        fs::create_dir_all(dir.join("m")).unwrap();
        let m = model(&dir.join("m"));
        assert_eq!(m.embed("parse config"), vec![1.0, 0.0, 0.0]);
        assert_eq!(m.embed("html"), vec![0.0, 1.0, 0.0]);
        assert_eq!(m.embed("nothing known"), vec![0.0, 0.0, 0.0]);
    }

    #[test]
    fn a_symbol_query_puts_the_definition_first_and_its_test_below() {
        let (index, embedder, _dir) = built("symbol");
        let hits = index.search("load_config", Some(&embedder), &opts(3, Mode::Hybrid));
        assert_eq!(hits[0].symbol, "load_config");
        assert_eq!((hits[0].path.as_str(), hits[0].start, hits[0].end), ("src/config.py", 1, 4));
        assert_eq!(hits[0].sig, "def load_config()");
        assert_eq!(hits[0].doc, "About load_config.");
        assert_ne!(hits[1].path, "tests/test_config.py", "{:?}", names(&hits));
        for pair in hits.windows(2) {
            assert!(pair[0].score >= pair[1].score);
        }
    }

    #[test]
    fn a_file_stem_that_matches_the_name_adds_half_as_much_again() {
        let dir = temp("stem");
        let twin = |path: &str| chunk(path, 1, 2, ChunkKind::Definition, "Gadget", (1, 2), "class Gadget: pass");
        // The two files hold the same text and the words of their names have the same length,
        // so BM25 ties and the earlier path ranks first; "gadgets" is a plural of the name,
        // which lifts its chunk by 1.5 * 3 * best instead of 3 * best.
        let index = SearchIndex::build(vec![twin("src/alpha.py"), twin("src/gadgets.py")], None, &dir).unwrap();
        let hits = index.search("Gadget", None, &opts(2, Mode::Words));
        assert_eq!(names(&hits), vec!["src/gadgets.py:Gadget", "src/alpha.py:Gadget"]);
        // Ranks 1 and 2 give 1/61 and 1/62; the file lift makes each 1.2 times that.
        let (first, second) = (1.2 / 61.0, 1.2 / 62.0);
        let (alpha, gadgets) = (first + 3.0 * first, second + 4.5 * first);
        assert!((f64::from(hits[0].score) - gadgets).abs() < 1e-6, "{} {gadgets}", hits[0].score);
        assert!((f64::from(hits[1].score) - alpha).abs() < 1e-6, "{} {alpha}", hits[1].score);
    }

    #[test]
    fn lifts_add_the_documented_amounts() {
        let (index, _embedder, _dir) = built("lifts");
        // Chunk numbers: 0 __init__, 1 load_config, 2 dump_config, 3 render_page, 4 escape,
        // 5 and 6 the parts of Server, 7 util module, 8 helper, 9 test.
        let close = |a: f64, b: f64| (a - b).abs() < 1e-12;

        // Files: the best chunk of each file gets 0.2 * best * (file sum / best file sum).
        let mut scores = BTreeMap::from([(1, 0.5), (2, 0.25), (3, 1.0)]);
        index.lift_files(&mut scores);
        assert!(close(scores[&1], 0.5 + 0.2 * 0.75), "{scores:?}");
        assert!(close(scores[&2], 0.25), "{scores:?}");
        assert!(close(scores[&3], 1.0 + 0.2), "{scores:?}");

        // Words: a keyword that is a path word adds best * keywords matched / keywords.
        let mut scores = BTreeMap::from([(1, 1.0), (3, 0.5), (9, 0.25)]);
        index.lift_stems(&mut scores, "config invoice", 1.0);
        assert!(close(scores[&1], 1.5), "{scores:?}");
        assert!(close(scores[&3], 0.5), "{scores:?}");
        assert!(close(scores[&9], 0.75), "{scores:?}");

        // Symbol: 3 * best for a chunk that defines the name, half as much again when the
        // file stem is the name; chunks that are not candidates enter when their file stem is.
        let mut scores = BTreeMap::from([(5, 1.0), (1, 1.0)]);
        let names = vec!["Server".to_string()];
        index.lift_definitions(&mut scores, &names, 3.0, &|f: &FileInfo| f.stem == "server");
        assert!(close(scores[&5], 1.0 + 4.5), "{scores:?}");
        assert!(close(scores[&1], 1.0), "{scores:?}");
        assert!(close(scores[&6], 4.5), "{scores:?}");
        assert_eq!(scores.len(), 3);

        // A name that only a module chunk defines, and a qualified one.
        let mut scores = BTreeMap::from([(0, 1.0)]);
        index.lift_definitions(&mut scores, &["Legacy".to_string()], 3.0, &|_: &FileInfo| false);
        assert!(close(scores[&0], 4.0), "{scores:?}");
        let mut scores = BTreeMap::from([(1, 1.0)]);
        index.lift_definitions(&mut scores, &["load_config".to_string()], 3.0, &|_: &FileInfo| false);
        assert!(close(scores[&1], 4.0), "{scores:?}");
    }

    #[test]
    fn a_natural_language_query_is_ranked_by_words_and_vectors() {
        let (index, embedder, _dir) = built("text");
        let hits = index.search("render the html template", Some(&embedder), &opts(3, Mode::Hybrid));
        assert_eq!(hits[0].symbol, "render_page");
        let words = index.search("render the html template", Some(&embedder), &opts(3, Mode::Words));
        assert_eq!(words[0].symbol, "render_page");
    }

    #[test]
    fn meaning_mode_ranks_by_vectors_alone() {
        let (index, embedder, _dir) = built("meaning");
        // "retry" shares no word with render.py or config.py, yet only server.py is on its axis.
        let hits = index.search("retry", Some(&embedder), &opts(2, Mode::Meaning));
        assert_eq!(hits.len(), 2);
        assert_eq!(hits[0].symbol, "Server");
        let hits = index.search("html", Some(&embedder), &opts(1, Mode::Meaning));
        assert_eq!(hits[0].symbol, "render_page");
        // No known word in the query: nothing to compare.
        assert!(index.search("zebra", Some(&embedder), &opts(3, Mode::Meaning)).is_empty());
        // No embedder: nothing to compare.
        assert!(index.search("html", None, &opts(3, Mode::Meaning)).is_empty());
    }

    #[test]
    fn words_mode_ignores_the_vectors() {
        let (index, embedder, _dir) = built("words");
        let hits = index.search("socket", Some(&embedder), &opts(5, Mode::Words));
        assert_eq!(hits[0].symbol, "Server");
        // "template" is a word only in render_page; "html" is not a word anywhere else.
        let hits = index.search("html", Some(&embedder), &opts(5, Mode::Words));
        assert_eq!(names(&hits), vec!["src/render.py:render_page"]);
    }

    #[test]
    fn hybrid_without_vectors_or_embedder_is_words() {
        let dir = temp("fallback");
        let embedder = model(&dir.join("m"));
        let plain = SearchIndex::build(corpus(), None, &dir.join("plain")).unwrap();
        let both = SearchIndex::build(corpus(), Some(&embedder), &dir.join("both")).unwrap();
        assert!(!plain.has_vectors() && both.has_vectors());
        assert!(!dir.join("plain").join(VECTORS_FILE).exists());
        for query in ["socket connect", "load_config", "render the html template"] {
            let words = plain.search(query, None, &opts(4, Mode::Words));
            assert!(!words.is_empty(), "{query}");
            assert_eq!(plain.search(query, Some(&embedder), &opts(4, Mode::Hybrid)), words, "no vectors: {query}");
            assert_eq!(both.search(query, None, &opts(4, Mode::Hybrid)), words, "no embedder: {query}");
            assert_eq!(both.search(query, Some(&embedder), &opts(4, Mode::Words)), words, "words mode: {query}");
        }
    }

    #[test]
    fn limit_caps_the_hits_and_zero_gives_none() {
        let (index, embedder, _dir) = built("limit");
        for limit in 1..=4 {
            let hits = index.search("config file parse", Some(&embedder), &opts(limit, Mode::Hybrid));
            assert_eq!(hits.len(), limit);
        }
        assert!(index.search("config", Some(&embedder), &opts(0, Mode::Hybrid)).is_empty());
        assert!(index.search("   ", Some(&embedder), &opts(5, Mode::Hybrid)).is_empty());
        assert!(index.search("zzzunknown", None, &opts(5, Mode::Hybrid)).is_empty());
        let few = index.search("html", None, &opts(50, Mode::Words));
        assert_eq!(few.len(), 1);
    }

    #[test]
    fn parts_of_one_definition_come_back_as_one_hit_with_the_best_score() {
        let (index, embedder, _dir) = built("collapse");
        let hits = index.search("retry connect socket", Some(&embedder), &opts(5, Mode::Words));
        let servers: Vec<&Hit> = hits.iter().filter(|h| h.symbol == "Server").collect();
        assert_eq!(servers.len(), 1);
        assert_eq!((servers[0].start, servers[0].end), (1, 12));
        // The second part repeats the three words, so it is the better chunk.
        assert_eq!((servers[0].chunk_start, servers[0].chunk_end), (6, 12));
    }

    #[test]
    fn module_chunks_report_their_own_lines_and_can_define_a_name() {
        let (index, _embedder, _dir) = built("module");
        let hits = index.search("Legacy", None, &opts(3, Mode::Words));
        assert_eq!(hits[0].path, "src/__init__.py");
        assert_eq!((hits[0].start, hits[0].end, hits[0].chunk_start, hits[0].chunk_end), (1, 2, 1, 2));
        assert_eq!((hits[0].symbol.as_str(), hits[0].sig.as_str()), ("", ""));
    }

    #[test]
    fn test_paths_count_less_except_in_meaning_mode() {
        let dir = temp("penalty");
        let embedder = model(&dir.join("model"));
        let chunks = vec![
            chunk("src/net.py", 1, 2, ChunkKind::Definition, "dial", (1, 2), "def dial():\n    socket"),
            chunk("tests/test_net.py", 1, 2, ChunkKind::Definition, "test_dial", (1, 2), "def test_dial():\n    socket html"),
        ];
        let index = SearchIndex::build(chunks, Some(&embedder), &dir.join("index")).unwrap();
        // The test chunk has both query words, so it is first by words and by vectors; a
        // test path counts 0.3 times, which puts it second, except when only vectors count.
        let words = index.search("socket html", Some(&embedder), &opts(2, Mode::Words));
        assert_eq!(names(&words), vec!["src/net.py:dial", "tests/test_net.py:test_dial"]);
        let hybrid = index.search("socket html", Some(&embedder), &opts(2, Mode::Hybrid));
        assert_eq!(names(&hybrid), vec!["src/net.py:dial", "tests/test_net.py:test_dial"]);
        let meaning = index.search("socket html", Some(&embedder), &opts(2, Mode::Meaning));
        assert_eq!(names(&meaning), vec!["tests/test_net.py:test_dial", "src/net.py:dial"]);
    }

    #[test]
    fn rerank_scales_paths_and_halves_repeats() {
        let (index, _embedder, _dir) = built("rerank");
        // Chunk 9 is in tests/, 0 in __init__.py, 1 to 4 in two files: config.py has 1 and 2,
        // render.py has 3 and 4.
        // A test path counts 0.3 times, __init__.py half, a repeat from one file half as much.
        let plain = index.rerank(vec![(1, 0.5), (9, 1.0), (0, 0.8)], 3, true);
        assert_eq!(plain.iter().map(|p| p.0).collect::<Vec<_>>(), vec![1, 0, 9]);
        assert_eq!(plain.iter().map(|p| p.1).collect::<Vec<_>>(), vec![0.5, 0.8 * 0.5, 0.3]);
        let unpenalised = index.rerank(vec![(1, 0.5), (9, 1.0), (0, 0.8)], 3, false);
        assert_eq!(unpenalised.iter().map(|p| p.0).collect::<Vec<_>>(), vec![9, 0, 1]);
        let repeats = index.rerank(vec![(1, 1.0), (2, 0.9), (3, 0.8), (4, 0.7)], 4, false);
        assert_eq!(repeats.iter().map(|p| p.0).collect::<Vec<_>>(), vec![1, 3, 2, 4]);
        assert_eq!(repeats.iter().map(|p| p.1).collect::<Vec<_>>(), vec![1.0, 0.8, 0.45, 0.35]);
        // Once `limit` are taken, an entry that cannot beat the worst of them ends the pass.
        let cut = index.rerank(vec![(1, 1.0), (3, 0.9), (2, 0.4)], 2, false);
        assert_eq!(cut.iter().map(|p| p.0).collect::<Vec<_>>(), vec![1, 3]);
    }

    #[test]
    fn results_are_the_same_every_time() {
        let (index, embedder, _dir) = built("repeat");
        for query in ["config", "load_config", "retry connect", "Server", "how to render a page"] {
            let first = index.search(query, Some(&embedder), &opts(5, Mode::Hybrid));
            for _ in 0..5 {
                assert_eq!(index.search(query, Some(&embedder), &opts(5, Mode::Hybrid)), first, "{query}");
            }
        }
    }

    #[test]
    fn build_then_open_gives_the_same_index() {
        let (built, embedder, dir) = built("roundtrip");
        let opened = SearchIndex::open(&dir.join("index")).unwrap();
        assert_eq!(opened.len(), corpus().len());
        assert!(!opened.is_empty() && opened.has_vectors());
        for mode in [Mode::Hybrid, Mode::Words, Mode::Meaning] {
            for query in ["config", "load_config", "retry connect", "Server", "how to render a page", "src config"] {
                assert_eq!(
                    opened.search(query, Some(&embedder), &opts(5, mode)),
                    built.search(query, Some(&embedder), &opts(5, mode)),
                    "{mode:?} {query}"
                );
            }
        }
    }

    #[test]
    fn the_chunk_text_is_not_stored() {
        let (_index, _embedder, dir) = built("notext");
        let mut stored = Vec::new();
        for entry in fs::read_dir(dir.join("index")).unwrap() {
            stored.extend(fs::read(entry.unwrap().path()).unwrap());
        }
        let find = |needle: &str| stored.windows(needle.len()).any(|w| w == needle.as_bytes());
        assert!(find("load_config"), "names are stored");
        assert!(!find("def load_config(path):"), "text is not");
        assert!(!find("import os\nimport sys"), "text is not");
    }

    #[test]
    fn an_empty_index_builds_opens_and_finds_nothing() {
        let dir = temp("empty");
        let embedder = model(&dir.join("m"));
        let index = SearchIndex::build(Vec::new(), Some(&embedder), &dir.join("index")).unwrap();
        assert!(index.is_empty() && !index.has_vectors());
        assert!(index.search("anything", Some(&embedder), &SearchOpts::default()).is_empty());
        let opened = SearchIndex::open(&dir.join("index")).unwrap();
        assert_eq!(opened.len(), 0);
    }

    #[test]
    fn opening_a_missing_or_damaged_folder_is_an_error() {
        let dir = temp("damaged");
        assert!(SearchIndex::open(&dir.join("nothing")).is_err());
        assert!(SearchIndex::open(&dir).is_err());
        let embedder = model(&dir.join("m"));
        let make = |name: &str| {
            let at = dir.join(name);
            SearchIndex::build(corpus(), Some(&embedder), &at).unwrap();
            at
        };

        let at = make("truncated-table");
        let bytes = fs::read(at.join(TABLE_FILE)).unwrap();
        fs::write(at.join(TABLE_FILE), &bytes[..bytes.len() / 2]).unwrap();
        assert!(SearchIndex::open(&at).is_err());

        let at = make("garbage-table");
        fs::write(at.join(TABLE_FILE), b"this is not an index").unwrap();
        assert!(SearchIndex::open(&at).is_err());

        let at = make("flipped-words");
        let mut bytes = fs::read(at.join(BM25_FILE)).unwrap();
        let middle = bytes.len() / 2;
        bytes[middle] ^= 0xFF;
        fs::write(at.join(BM25_FILE), &bytes).unwrap();
        assert!(SearchIndex::open(&at).is_err());

        let at = make("missing-words");
        fs::remove_file(at.join(BM25_FILE)).unwrap();
        assert!(SearchIndex::open(&at).is_err());

        let at = make("missing-vectors");
        fs::remove_file(at.join(VECTORS_FILE)).unwrap();
        assert!(SearchIndex::open(&at).is_err());

        let at = make("truncated-vectors");
        let bytes = fs::read(at.join(VECTORS_FILE)).unwrap();
        fs::write(at.join(VECTORS_FILE), &bytes[..bytes.len() - 5]).unwrap();
        assert!(SearchIndex::open(&at).is_err());

        let at = make("other-table");
        let other = temp("damaged-other");
        SearchIndex::build(corpus()[..3].to_vec(), None, &other).unwrap();
        fs::copy(other.join(TABLE_FILE), at.join(TABLE_FILE)).unwrap();
        assert!(SearchIndex::open(&at).is_err());
    }

    /// The corpus with other texts: the same table apart from the hashes, other words and
    /// other vectors. Files of this build can stand in for files of the first one.
    fn other_corpus() -> Vec<Chunk> {
        let mut chunks = corpus();
        for c in &mut chunks {
            c.text.push_str(" socket retry");
        }
        chunks
    }

    #[test]
    fn a_file_from_another_build_of_the_same_size_is_refused() {
        let dir = temp("swapped");
        let embedder = model(&dir.join("m"));
        for file in [VECTORS_FILE, BM25_FILE, TABLE_FILE] {
            let (home, away) = (dir.join(format!("home-{file}")), dir.join(format!("away-{file}")));
            SearchIndex::build(corpus(), Some(&embedder), &home).unwrap();
            SearchIndex::build(other_corpus(), Some(&embedder), &away).unwrap();
            assert!(SearchIndex::open(&home).is_ok(), "{file}");
            fs::copy(away.join(file), home.join(file)).unwrap();
            assert!(SearchIndex::open(&home).is_err(), "{file} of another build was accepted");
        }
    }

    #[test]
    fn a_changed_table_or_vectors_file_is_refused() {
        let dir = temp("changed");
        let embedder = model(&dir.join("m"));
        let at = dir.join("index");
        SearchIndex::build(corpus(), Some(&embedder), &at).unwrap();
        assert!(SearchIndex::open(&at).is_ok());

        // The first line of one chunk moves by one, a change that leaves the table sound.
        let mut table: Table = load(&at.join(TABLE_FILE)).unwrap();
        let row = table.rows.iter_mut().find(|r| r.end > r.start).unwrap();
        row.start += 1;
        save(&at.join(TABLE_FILE), &table).unwrap();
        let error = SearchIndex::open(&at).unwrap_err();
        assert!(error.to_string().contains("is damaged"), "{error}");

        // One bit of one vector flips, a change that leaves the file well formed.
        SearchIndex::build(corpus(), Some(&embedder), &at).unwrap();
        let mut bytes = fs::read(at.join(VECTORS_FILE)).unwrap();
        let last = bytes.len() - 1;
        bytes[last] ^= 0x01;
        fs::write(at.join(VECTORS_FILE), &bytes).unwrap();
        let error = SearchIndex::open(&at).unwrap_err();
        assert!(error.to_string().contains("is damaged"), "{error}");
    }

    #[test]
    fn a_build_that_fails_leaves_an_existing_index_as_it_was() {
        let dir = temp("failed");
        let embedder = model(&dir.join("m"));
        let at = dir.join("index");
        SearchIndex::build(corpus(), Some(&embedder), &at).unwrap();
        let files = |at: &Path| {
            [TABLE_FILE, BM25_FILE, VECTORS_FILE].map(|f| {
                let bytes = fs::read(at.join(f)).unwrap();
                (bytes.len(), hash64(&bytes))
            })
        };
        let before = files(&at);

        // A chunk whose first line is after its last cannot be in a sound table.
        let mut bad = corpus();
        bad[0].start = bad[0].end + 1;
        assert!(SearchIndex::build(bad, Some(&embedder), &at).is_err());
        assert_eq!(files(&at), before, "the failed build changed the files");
        assert!(SearchIndex::open(&at).is_ok());
        let left: Vec<_> = fs::read_dir(&at).unwrap().map(|e| e.unwrap().file_name()).collect();
        assert_eq!(left.len(), 3, "{left:?}");
    }

    #[test]
    fn builds_racing_into_one_folder_never_leave_a_mixture() {
        let dir = temp("racing");
        let embedder = model(&dir.join("m"));
        let (a, b) = (corpus(), other_corpus());
        // What an index answers: words from its word index, meaning from its vectors.
        let answers = |index: &SearchIndex| {
            (index.search("retry", Some(&embedder), &opts(5, Mode::Words)), index.search("retry", Some(&embedder), &opts(5, Mode::Meaning)))
        };
        let alone = |name: &str, chunks: &Vec<Chunk>| answers(&SearchIndex::build(chunks.clone(), Some(&embedder), &dir.join(name)).unwrap());
        let (only_a, only_b) = (alone("only-a", &a), alone("only-b", &b));
        assert_ne!(only_a, only_b);
        let at = dir.join("shared");
        for round in 0..20 {
            std::thread::scope(|scope| {
                for chunks in [&a, &b] {
                    let (at, embedder) = (&at, &embedder);
                    scope.spawn(move || {
                        let _ = SearchIndex::build(chunks.clone(), Some(embedder), at);
                    });
                }
            });
            // Either the folder is refused, or it is one of the two builds, whole.
            if let Ok(index) = SearchIndex::open(&at) {
                let found = answers(&index);
                assert!(found == only_a || found == only_b, "round {round}: a mixture of the two builds");
            }
        }
    }

    #[test]
    fn an_embedder_of_another_model_is_not_used_for_the_vectors() {
        let (index, embedder, dir) = built("othermodel");
        let (a, b, c): (&[f32], &[f32], &[f32]) = (&[1.0, 0.0, 0.0], &[0.0, 1.0, 0.0], &[0.0, 0.0, 1.0]);
        // The same words and vectors of the same length, pointing along other axes.
        let rotated = model_of(
            &dir.join("rotated"),
            3,
            &[("parse", c), ("config", c), ("file", c), ("render", a), ("html", a), ("template", a), ("socket", b), ("connect", b), ("retry", b)],
        );
        assert_eq!(rotated.dim(), embedder.dim());
        assert!(!index.search("retry", Some(&embedder), &opts(2, Mode::Meaning)).is_empty());
        assert!(index.search("retry", Some(&rotated), &opts(2, Mode::Meaning)).is_empty());
        let words = index.search("socket connect", None, &opts(4, Mode::Words));
        assert!(!words.is_empty());
        assert_eq!(index.search("socket connect", Some(&rotated), &opts(4, Mode::Hybrid)), words);
    }

    #[test]
    fn an_embedder_with_vectors_of_another_length_is_not_used() {
        let (index, embedder, dir) = built("otherdim");
        let wide = model_of(
            &dir.join("wide"),
            4,
            &[("retry", &[1.0, 0.0, 0.0, 0.0]), ("socket", &[0.0, 1.0, 0.0, 0.0]), ("html", &[0.0, 0.0, 1.0, 0.0])],
        );
        assert_ne!(wide.dim(), embedder.dim());
        assert!(index.search("retry", Some(&wide), &opts(3, Mode::Meaning)).is_empty());
        let words = index.search("retry socket", None, &opts(3, Mode::Words));
        assert!(!words.is_empty());
        assert_eq!(index.search("retry socket", Some(&wide), &opts(3, Mode::Hybrid)), words);
    }

    #[test]
    fn the_callers_alpha_is_cut_to_0_and_1_and_its_ends_are_the_single_retrievers() {
        let (index, embedder, _dir) = built("alpha");
        let with = |query: &str, mode: Mode, alpha: Option<f32>| index.search(query, Some(&embedder), &SearchOpts { limit: 5, mode, alpha });
        let text = "render the html template";
        let (words, meaning) = (with(text, Mode::Words, None), with(text, Mode::Meaning, None));
        assert!(!words.is_empty() && !meaning.is_empty());
        // 0 weighs the vectors nothing and 1 weighs the words nothing; beyond them is cut to them.
        assert_eq!(with(text, Mode::Hybrid, Some(0.0)), words);
        assert_eq!(with(text, Mode::Hybrid, Some(-3.0)), words);
        assert_eq!(with(text, Mode::Hybrid, Some(1.0)), meaning);
        assert_eq!(with(text, Mode::Hybrid, Some(7.5)), meaning);
        // Without an alpha a text query weighs the vectors 0.5 and a symbol query 0.3.
        assert_eq!(with(text, Mode::Hybrid, Some(0.5)), with(text, Mode::Hybrid, None));
        assert_eq!(with("load_config", Mode::Hybrid, Some(0.3)), with("load_config", Mode::Hybrid, None));
    }

    /// The parts of an index built from the corpus, read back, for a test to change.
    fn parts(at: &Path) -> (Table, Bm25, Option<Vectors>) {
        (load(&at.join(TABLE_FILE)).unwrap(), load(&at.join(BM25_FILE)).unwrap(), Some(Vectors::open(&at.join(VECTORS_FILE)).unwrap()))
    }

    #[test]
    fn from_parts_refuses_a_table_that_does_not_hold_together() {
        let dir = temp("parts");
        let embedder = model(&dir.join("m"));
        let at = dir.join("index");
        SearchIndex::build(corpus(), Some(&embedder), &at).unwrap();
        let module = parts(&at).0.rows.iter().position(|r| r.kind == KIND_MODULE).unwrap() as u32;
        let definition = parts(&at).0.rows.iter().position(|r| r.kind == KIND_DEFINITION).unwrap() as u32;

        let (table, bm25, vectors) = parts(&at);
        assert!(SearchIndex::from_parts(table, bm25, vectors).is_ok());

        let cases: Vec<(&str, Box<dyn Fn(&mut Table)>)> = vec![
            ("chunk 0 of the table is not valid", Box::new(|t: &mut Table| t.rows[0].start = t.rows[0].end + 1)),
            ("chunk 0 of the table is not valid", Box::new(|t: &mut Table| t.rows[0].def_start = t.rows[0].def_end + 1)),
            ("chunk 0 of the table is not valid", Box::new(|t: &mut Table| t.rows[0].kind = KIND_MODULE + 1)),
            ("chunk 0 of the table is not valid", Box::new(|t: &mut Table| t.rows[0].path = t.paths.len() as u32)),
            ("chunk 0 of the table is not valid", Box::new(|t: &mut Table| t.rows[0].symbol = t.strings.len() as u32)),
            ("chunk 0 of the table is not valid", Box::new(|t: &mut Table| t.rows[0].sig = t.strings.len() as u32)),
            ("chunk 0 of the table is not valid", Box::new(|t: &mut Table| t.rows[0].doc = t.strings.len() as u32)),
            ("does not start with the empty string", Box::new(|t: &mut Table| t.strings[0] = "x".to_string())),
            (
                "does not point at a module chunk",
                Box::new(move |t: &mut Table| t.modules.push(ModuleNames { chunk: definition, general: Vec::new(), sql: Vec::new() })),
            ),
            (
                "does not point at a module chunk",
                Box::new(|t: &mut Table| t.modules.push(ModuleNames { chunk: t.rows.len() as u32, general: Vec::new(), sql: Vec::new() })),
            ),
            ("probe vector does not match", Box::new(|t: &mut Table| t.probe.truncate(1))),
            ("vectors do not match", Box::new(|t: &mut Table| t.dim += 1)),
            ("word index does not match", Box::new(|t: &mut Table| t.format += 1)),
        ];
        assert!(module != definition);
        for (expected, change) in &cases {
            let (mut table, bm25, vectors) = parts(&at);
            change(&mut table);
            let error = SearchIndex::from_parts(table, bm25, vectors).unwrap_err();
            assert!(error.to_string().contains(expected), "{expected}: {error}");
        }
        // The vectors missing while the table says there are some.
        let (table, bm25, _) = parts(&at);
        assert!(SearchIndex::from_parts(table, bm25, None).unwrap_err().to_string().contains("vectors are missing"));
        // A real module entry is accepted.
        let (mut table, bm25, vectors) = parts(&at);
        table.modules.push(ModuleNames { chunk: module, general: Vec::new(), sql: Vec::new() });
        assert!(SearchIndex::from_parts(table, bm25, vectors).is_ok());
    }

    #[test]
    fn a_definition_cut_into_many_parts_does_not_use_up_the_pool() {
        let dir = temp("pool");
        let text = "needle needle needle needle needle needle";
        // 30 parts of one definition that repeat the word, and 6 small functions with it once.
        let mut chunks: Vec<Chunk> =
            (0..30).map(|i| chunk("src/table.py", 10 * i + 1, 10 * i + 9, ChunkKind::Part, "TABLE", (1, 400), text)).collect();
        for i in 0..6 {
            chunks.push(chunk(&format!("src/f{i}.py"), 1, 2, ChunkKind::Definition, &format!("f{i}"), (1, 2), "def f():\n    needle"));
        }
        let index = SearchIndex::build(chunks, None, &dir).unwrap();
        // Five times the limit is 25 chunks, and all 25 are parts of TABLE.
        let hits = index.search("needle", None, &opts(5, Mode::Words));
        assert_eq!(hits.len(), 5, "{:?}", names(&hits));
        let distinct: HashSet<&str> = hits.iter().map(|h| h.symbol.as_str()).collect();
        assert_eq!(distinct.len(), 5);
        // More are asked for than there are definitions: all seven come back.
        assert_eq!(index.search("needle", None, &opts(20, Mode::Words)).len(), 7);
    }

    #[test]
    fn the_documentation_above_a_declaration_finds_the_definition() {
        let dir = temp("doc");
        let text = "use std::fmt;\n\n/// Adds one.\n#[inline]\npub fn add(x: u32) -> u32 {\n    x + 1\n}\n\n/// Takes one away.\npub fn sub(x: u32) -> u32 {\n    x - 1\n}\n";
        let facts = lxr_extract::extract("src/x.rs", lxr_core::Lang::Rust, text);
        let chunks = crate::chunk_file(&facts, text);
        // The comment lines are not lines of the function, so they are a chunk of their own.
        assert!(chunks.iter().any(|c| c.kind == ChunkKind::Module && c.text.contains("Adds one.")), "{chunks:?}");
        let add = chunks.iter().find(|c| c.symbol == "add").unwrap();
        assert_eq!((add.doc.as_str(), add.text.contains("Adds one.")), ("Adds one.", false));
        let index = SearchIndex::build(chunks, None, &dir).unwrap();
        let hits = index.search("adds one", None, &opts(3, Mode::Words));
        assert!(hits.iter().any(|h| h.symbol == "add"), "{:?}", names(&hits));
        let hits = index.search("takes one away", None, &opts(3, Mode::Words));
        assert!(hits.iter().any(|h| h.symbol == "sub"), "{:?}", names(&hits));
    }

    #[test]
    fn the_indexed_text_adds_a_documentation_line_only_where_it_is_missing() {
        let with_doc = |kind: ChunkKind, start: u32, doc: &str, text: &str| {
            let mut c = chunk("src/x.py", start, start + 2, kind, "f", (4, 9), text);
            c.doc = doc.to_string();
            c
        };
        let def = ChunkKind::Definition;
        // Missing from the chunk that begins the definition: put in front.
        assert_eq!(indexed_text(&with_doc(def, 4, "Adds one.", "fn f() {}")), "Adds one.\nfn f() {}");
        // Already in the chunk, also when the text breaks the line in another place or is cut.
        assert!(matches!(indexed_text(&with_doc(def, 4, "Parse the config.", "def f():\n    \"\"\"Parse the\n    config.\"\"\"")), Cow::Borrowed(_)));
        assert!(matches!(indexed_text(&with_doc(def, 4, "Parse the conf...", "def f():\n    \"\"\"Parse the config file.\"\"\"")), Cow::Borrowed(_)));
        // A chunk in the middle of the definition, a module chunk and an empty line add nothing.
        assert!(matches!(indexed_text(&with_doc(ChunkKind::Part, 6, "Adds one.", "x")), Cow::Borrowed(_)));
        assert!(matches!(indexed_text(&with_doc(ChunkKind::Module, 4, "Adds one.", "x")), Cow::Borrowed(_)));
        assert!(matches!(indexed_text(&with_doc(def, 4, "", "x")), Cow::Borrowed(_)));
    }

    #[test]
    fn a_digest_does_not_depend_on_how_the_bytes_are_cut() {
        let bytes: Vec<u8> = (0..3 * DIGEST_BLOCK + 123).map(|i| (i * 7 % 251) as u8).collect();
        let digest_of = |pieces: usize| {
            let mut digest = Digest::new();
            for part in bytes.chunks(pieces) {
                digest.put(part);
            }
            digest.finish()
        };
        let whole = digest_of(bytes.len());
        for pieces in [1, 7, DIGEST_BLOCK - 1, DIGEST_BLOCK, DIGEST_BLOCK + 1] {
            assert_eq!(digest_of(pieces), whole, "pieces of {pieces}");
        }
        let mut changed = bytes.clone();
        changed[DIGEST_BLOCK] ^= 1;
        let mut digest = Digest::new();
        digest.put(&changed);
        assert_ne!(digest.finish(), whole);
    }

    #[test]
    fn verify_refuses_a_file_that_is_not_the_one_the_table_describes() {
        let dir = temp("verify");
        let path = dir.join("file.bin");
        let bytes: Vec<u8> = (0..100_000u32).map(|i| (i % 253) as u8).collect();
        fs::write(&path, &bytes).unwrap();
        let (size, hash) = file_digest(&path).unwrap();
        assert_eq!(size, bytes.len() as u64);
        verify(&path, size, hash).unwrap();
        let mut flipped = bytes.clone();
        flipped[70_000] ^= 0x10;
        fs::write(&path, &flipped).unwrap();
        assert!(verify(&path, size, hash).is_err());
        fs::write(&path, &bytes[..bytes.len() - 1]).unwrap();
        assert!(verify(&path, size, hash).is_err());
        fs::remove_file(&path).unwrap();
        assert!(verify(&path, size, hash).is_err());
    }

    #[test]
    fn write_checked_describes_the_bytes_it_leaves_at_the_path() {
        let dir = temp("checked");
        let path = dir.join("file.bin");
        fs::write(&path, b"something else that was already here").unwrap();
        let figures = write_checked(&path, |temp| fs::write(temp, b"the new bytes")).unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"the new bytes");
        assert_eq!(figures, file_digest(&path).unwrap());
        // A write that fails leaves the file as it was and no temporary file behind.
        let failed = write_checked(&path, |temp| {
            fs::write(temp, b"half")?;
            Err(io::Error::new(io::ErrorKind::Other, "disk full"))
        });
        assert!(failed.is_err());
        assert_eq!(fs::read(&path).unwrap(), b"the new bytes");
        let left: Vec<_> = fs::read_dir(&dir).unwrap().map(|e| e.unwrap().file_name()).collect();
        assert_eq!(left.len(), 1, "{left:?}");
    }

    #[test]
    fn dropping_an_index_lets_go_of_its_files() {
        let (index, _embedder, dir) = built("release");
        assert!(index.has_vectors());
        drop(index);
        fs::remove_dir_all(dir.join("index")).unwrap();
        assert!(!dir.join("index").exists());
    }
}
