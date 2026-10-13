//! The bank: notes, their indexes and their files.

use crate::dates::{iso_date, parse_span, unix_now};
use crate::entities::{dedupe, extract, fold, EntityTable};
use crate::graph::{temporal_neighbors, Graph, LinkKind, SEMANTIC_LIMIT, TEMPORAL_LIMIT};
use crate::log::{forget_line, id_for, note_line, replay};
use crate::split::{collapse, split_facts, whole};
use crate::store::{build_bm25, load_snapshot, save_snapshot, Parts, VecStore, INDEX_FILE, LOG_FILE, VECTORS_FILE};
use crate::{
    Anchor, BankStats, CodeView, Kind, Note, RecallOpts, Recalled, Reflection, RetainOpts, RetainOutcome, Retained, Strategies,
    LINK_THRESHOLD, NEAR_DUPLICATE,
};
use lxr_core::hash64;
use lxr_text::{Bm25, Embedder};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::time::SystemTime;

/// Notes of one observation: it is built when this many notes share an entity and tags.
const OBSERVATION_FROM: usize = 3;
/// An observation's text is cut to this many characters.
const OBSERVATION_CHARS: usize = 600;
/// Most lines kept in a note's history.
const HISTORY_LIMIT: usize = 50;
/// Most anchors kept on a note.
const ANCHOR_LIMIT: usize = 8;
/// Most source ids an observation lists: the newest notes of its group. Its `proof_count`
/// holds the whole count, and the cap keeps a line of the log the same size however large the
/// group grows.
const SOURCE_LIMIT: usize = 50;
/// The file whose lock a bank holds while it changes `notes.jsonl`.
const LOCK_FILE: &str = "notes.lock";
/// Punctuation that opens or ends a sentence. It is not part of what a fact says; a symbol
/// such as `+`, `#`, `%` or a bracket is.
const SENTENCE_MARKS: &[char] = &['.', ',', ';', ':', '!', '?', '\u{2026}'];

/// The parts of a bank that a recall may have to complete while it reads: the vectors, the
/// links that come from them and the keyword index.
pub(crate) struct Derived {
    pub vecs: VecStore,
    pub graph: Graph,
    /// The keyword index; `None` while it is out of date.
    pub bm25: Option<Bm25>,
    /// Vectors or links were made while reading, so the caches on disk are out of date.
    pub changed: bool,
}

/// Everything about the notes themselves.
pub(crate) struct Core {
    /// Notes by slot; `None` once forgotten. A slot is never reused.
    pub slots: Vec<Option<Note>>,
    /// Slot of every note that was not forgotten.
    pub by_id: HashMap<String, u32>,
    pub next_seq: u64,
    pub ents: EntityTable,
    /// Entity ids of the note in each slot.
    pub note_ents: Vec<Vec<u32>>,
    /// `(time, slot)` of the notes that get temporal links. Built on first use.
    times: BTreeSet<(i64, u32)>,
    /// `(kind, hash of the normalised text)` to slot. Built on first use.
    dup: HashMap<(u8, u64), u32>,
    /// `(entity, tag key)` to the slot of the observation that lists the entity as a subject
    /// for that tag set. An observation has an entry for each of its subjects. Built on first use.
    obs: HashMap<(u32, String), u32>,
    lazy_ready: bool,
}

/// The log file and what is known about its end.
struct LogFile {
    path: PathBuf,
    file: Option<File>,
    /// Bytes in the file, as far as this bank knows.
    len: u64,
    /// The modification time the file had when this bank last read or wrote it. With `len` it
    /// tells [`Bank::refresh`] whether another bank has written since.
    modified: Option<SystemTime>,
    /// True when the file is empty or ends in a line break.
    ends_clean: bool,
}

impl LogFile {
    fn append(&mut self, bytes: &[u8]) -> io::Result<()> {
        let mut file = match self.file.take() {
            Some(file) => file,
            None => OpenOptions::new().create(true).append(true).open(&self.path)?,
        };
        let result = self.write_block(&mut file, bytes);
        self.file = Some(file);
        if result.is_ok() {
            self.modified = fs::metadata(&self.path).ok().and_then(|m| m.modified().ok());
        }
        result
    }

    /// Writes `bytes` as one block to `out`, after a line break when the file does not end in
    /// one. When the write fails part of the block may be on disk, so the next block starts on
    /// a fresh line and cannot be glued to the fragment.
    fn write_block(&mut self, out: &mut impl Write, bytes: &[u8]) -> io::Result<()> {
        let mut block = Vec::with_capacity(bytes.len() + 1);
        if !self.ends_clean {
            block.push(b'\n');
        }
        block.extend_from_slice(bytes);
        if let Err(error) = out.write_all(&block).and_then(|()| out.flush()) {
            self.ends_clean = false;
            return Err(error);
        }
        self.len += block.len() as u64;
        self.ends_clean = true;
        Ok(())
    }
}

/// The exclusive lock on `notes.lock` that a bank holds while it changes the log. A bank on
/// the same folder, in this process or another, waits for it. The lock is on a file of its
/// own because a lock on the log would also keep other banks from reading it.
///
/// A file system that cannot lock leaves the bank without one: it still compares the length of
/// the log with what it knows before it writes, which narrows the window but does not close it.
struct LogLock(Option<File>);

impl LogLock {
    fn take(dir: &Path) -> LogLock {
        let file = OpenOptions::new().create(true).write(true).truncate(false).open(dir.join(LOCK_FILE)).ok();
        LogLock(file.filter(|file| file.lock().is_ok()))
    }
}

impl Drop for LogLock {
    fn drop(&mut self) {
        if let Some(file) = &self.0 {
            let _ = file.unlock();
        }
    }
}

/// Why [`Bank::flush`] failed. The two kinds put different things at risk, so a caller can
/// treat them differently.
#[derive(Debug)]
pub enum FlushError {
    /// `notes.jsonl` could not be made durable, or an earlier write to it failed. A note that
    /// was reported as kept may be lost.
    Log(io::Error),
    /// A cache file (`vectors.bin` or `index.bin`) could not be written. Every note is safe in
    /// the log, and the next open rebuilds the caches from it.
    Cache(io::Error),
}

impl FlushError {
    /// The underlying error.
    pub fn io_error(&self) -> &io::Error {
        match self {
            FlushError::Log(error) | FlushError::Cache(error) => error,
        }
    }

    /// True when the log could not be made durable: the failure that can lose a note.
    pub fn is_log(&self) -> bool {
        matches!(self, FlushError::Log(_))
    }
}

impl fmt::Display for FlushError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FlushError::Log(error) => write!(f, "the notes could not be made durable: {error}"),
            FlushError::Cache(error) => write!(f, "the cache files could not be written: {error}"),
        }
    }
}

impl std::error::Error for FlushError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(self.io_error())
    }
}

impl From<FlushError> for io::Error {
    fn from(error: FlushError) -> io::Error {
        let kind = error.io_error().kind();
        io::Error::new(kind, error)
    }
}

/// What a bank holds in memory, read from its folder.
struct Loaded {
    core: Core,
    derived: Derived,
    /// The caches on disk do not describe this state.
    dirty: bool,
    /// Size and modification time of the log when it was read.
    len: u64,
    modified: Option<SystemTime>,
    ends_clean: bool,
}

/// Reads the state of the bank in `dir`. When `index.bin` and `vectors.bin` are current, nothing
/// is parsed or embedded: the snapshot is read and the vectors are memory-mapped. Otherwise the
/// state is rebuilt from `notes.jsonl`, except for the vectors, which need an embedder.
fn load(dir: &Path) -> io::Result<Loaded> {
    let log_path = dir.join(LOG_FILE);
    let meta = fs::metadata(&log_path).ok();
    let len = meta.as_ref().map_or(0, |m| m.len());
    let modified = meta.and_then(|m| m.modified().ok());
    let ends_clean = ends_with_newline(&log_path, len);
    let (core, derived, dirty) = if len == 0 {
        (Core::blank(), Derived { vecs: VecStore::default(), graph: Graph::default(), bm25: None, changed: false }, false)
    } else if let Some((core, derived)) = from_snapshot(dir, len, modified) {
        (core, derived, false)
    } else {
        let bytes = fs::read(&log_path)?;
        let (core, graph) = Core::from_log(&bytes);
        let slots = core.slots.len();
        (core, Derived { vecs: VecStore::empty(0, slots), graph, bm25: None, changed: false }, true)
    };
    Ok(Loaded { core, derived, dirty, len, modified, ends_clean })
}

/// A folder of notes for one repository.
pub struct Bank {
    dir: PathBuf,
    pub(crate) core: Core,
    pub(crate) derived: Mutex<Derived>,
    log: LogFile,
    /// The caches on disk are out of date.
    pub(crate) dirty: bool,
    /// Writing the log failed: the memory is ahead of the file, so no cache may be written.
    failed: bool,
    /// The last [`Bank::flush`] returned a cache error and nothing has changed since, so the
    /// caller already knows. Dropping the bank does not report it a second time.
    cache_reported: bool,
    /// Opened by [`Bank::open_read_only`]: nothing is written, not the log, not the caches, not
    /// when the bank is dropped.
    read_only: bool,
}

impl Bank {
    /// Opens the bank in `dir`, creating the folder and an empty bank when missing.
    ///
    /// When `index.bin` and `vectors.bin` are current, nothing is parsed or embedded: the
    /// snapshot is read and the vectors are memory-mapped. Otherwise the bank is rebuilt from
    /// `notes.jsonl`, except for the vectors, which need an embedder and are made the first
    /// time one is passed to `retain`, `recall` or `reflect`.
    pub fn open(dir: &Path) -> io::Result<Bank> {
        fs::create_dir_all(dir)?;
        Bank::load_from(dir, false)
    }

    fn load_from(dir: &Path, read_only: bool) -> io::Result<Bank> {
        let Loaded { core, derived, dirty, len, modified, ends_clean } = load(dir)?;
        let log = LogFile { path: dir.join(LOG_FILE), file: None, len, modified, ends_clean };
        let derived = Mutex::new(derived);
        Ok(Bank { dir: dir.to_path_buf(), core, derived, log, dirty, failed: false, cache_reported: false, read_only })
    }

    /// Opens the bank in `dir` for reading only. The folder is not created when it is missing (the
    /// bank is then empty), and nothing is ever written to it: [`Bank::retain`] and
    /// [`Bank::forget`] give a `PermissionDenied` error, [`Bank::flush`] and the drop of the bank
    /// leave the caches as they are even when they are out of date, and a recall that has to
    /// embed notes keeps the vectors in memory only. For a caller that must not change the folder
    /// at all, such as a loadout that an agent runs before it starts.
    pub fn open_read_only(dir: &Path) -> io::Result<Bank> {
        Bank::load_from(dir, true)
    }

    /// The error of a change asked of a bank that was opened for reading only.
    fn writable(&self) -> io::Result<()> {
        if self.read_only {
            return Err(io::Error::new(io::ErrorKind::PermissionDenied, "the bank was opened for reading only"));
        }
        Ok(())
    }

    /// Catches up with notes that another bank, in this process or another, has written since
    /// this one last looked. A bank that is held for a long time (a server) calls this before it
    /// answers, instead of being opened again.
    ///
    /// It costs one look at the log's size and modification time. Only when either has changed
    /// are the notes read again: from the caches the other bank left when they are current, else
    /// from the log. True when the notes were read again. A bank that has to read them again and
    /// cannot keeps what it had and gives the error; the next call tries again.
    ///
    /// The change is not locked against: a bank that writes at the same moment may be caught
    /// part way, and the next call finishes the catch-up. Writing needs no call to this, because
    /// [`Bank::retain`] and [`Bank::forget`] catch up under the lock themselves.
    pub fn refresh(&mut self) -> io::Result<bool> {
        let meta = fs::metadata(&self.log.path).ok();
        let len = meta.as_ref().map_or(0, |m| m.len());
        let modified = meta.and_then(|m| m.modified().ok());
        if !self.failed && len == self.log.len && modified == self.log.modified {
            return Ok(false);
        }
        let Loaded { core, derived, dirty, len, modified, ends_clean } = load(&self.dir)?;
        self.core = core;
        *unlock(&mut self.derived) = derived;
        self.log.len = len;
        self.log.modified = modified;
        self.log.ends_clean = ends_clean;
        self.dirty = dirty;
        self.failed = false;
        self.cache_reported = false;
        Ok(true)
    }

    /// Remembers the facts in `text`.
    ///
    /// The text is cut into facts (unless `opts.split` is off); each fact becomes a note, or
    /// reinforces a note the bank already holds. The notes are appended to `notes.jsonl`
    /// before this returns. The caches are written by [`Bank::flush`] and when the bank is
    /// dropped.
    ///
    /// Another bank on the same folder may have written since this one last looked. The log is
    /// locked while this call works, and anything the other bank appended is read first, so
    /// two banks never hand out one id and neither undoes the other's forget. When the call
    /// fails, the notes it made are not kept in memory either.
    pub fn retain(
        &mut self,
        text: &str,
        opts: &RetainOpts,
        code: &dyn CodeView,
        embedder: Option<&Embedder>,
    ) -> io::Result<Vec<Retained>> {
        self.writable()?;
        let _lock = LogLock::take(&self.dir);
        self.catch_up()?;
        let derived = unlock(&mut self.derived);
        let (out, lines) = match self.core.retain(derived, text, opts, code, embedder) {
            Ok(done) => done,
            Err(error) => {
                self.restore();
                return Err(error);
            }
        };
        if !lines.is_empty() {
            self.dirty = true;
            if let Err(error) = self.append(&lines) {
                self.restore();
                return Err(error);
            }
        }
        Ok(out)
    }

    /// Recalls the notes that best answer `query`: meaning, words, links and time, fused,
    /// boosted by recency, time and proof count, and demoted when the code a note talks about
    /// has changed. Directives that pass the filters come first, within the half of the limit
    /// and of the token allowance they may use while ranked notes are waiting.
    pub fn recall(
        &self,
        query: &str,
        opts: &RecallOpts,
        code: &dyn CodeView,
        embedder: Option<&Embedder>,
    ) -> Vec<Recalled> {
        crate::recall::recall(self, query, opts, code, embedder, Strategies::ALL)
    }

    /// Like [`Bank::recall`], with only some of the four strategies feeding the fusion. For
    /// measuring what each of them contributes.
    pub fn recall_with(
        &self,
        query: &str,
        opts: &RecallOpts,
        code: &dyn CodeView,
        embedder: Option<&Embedder>,
        strategies: Strategies,
    ) -> Vec<Recalled> {
        crate::recall::recall(self, query, opts, code, embedder, strategies)
    }

    /// Recalls with one budget step more and sorts the result into directives, observations
    /// and facts, with the entities the returned notes share.
    pub fn reflect(
        &self,
        query: &str,
        opts: &RecallOpts,
        code: &dyn CodeView,
        embedder: Option<&Embedder>,
    ) -> Reflection {
        crate::recall::reflect(self, query, opts, code, embedder)
    }

    /// Forgets a note: a tombstone is appended and the note leaves every index. False when
    /// there is no such note. Like [`Bank::retain`], it reads what another bank appended first.
    pub fn forget(&mut self, id: &str) -> io::Result<bool> {
        self.writable()?;
        let _lock = LogLock::take(&self.dir);
        self.catch_up()?;
        let derived = unlock(&mut self.derived);
        let lines = match self.core.forget(derived, id, unix_now()) {
            Ok(Some(lines)) => lines,
            Ok(None) => return Ok(false),
            Err(error) => {
                self.restore();
                return Err(error);
            }
        };
        self.dirty = true;
        if let Err(error) = self.append(&lines) {
            self.restore();
            return Err(error);
        }
        Ok(true)
    }

    /// The note with this id.
    pub fn get(&self, id: &str) -> Option<&Note> {
        let slot = *self.core.by_id.get(id)?;
        self.core.slots.get(slot as usize)?.as_ref()
    }

    /// All notes, newest first.
    pub fn notes(&self) -> Vec<&Note> {
        let mut all: Vec<(usize, &Note)> = self.core.slots.iter().enumerate().filter_map(|(i, n)| n.as_ref().map(|n| (i, n))).collect();
        all.sort_by(|a, b| b.1.created_at.cmp(&a.1.created_at).then(b.0.cmp(&a.0)));
        all.into_iter().map(|(_, n)| n).collect()
    }

    /// Number of notes.
    pub fn len(&self) -> usize {
        self.core.by_id.len()
    }

    /// True when the bank holds no note.
    pub fn is_empty(&self) -> bool {
        self.core.by_id.is_empty()
    }

    /// What the bank holds.
    pub fn stats(&self) -> BankStats {
        let mut stats = BankStats { notes: self.core.by_id.len(), ..BankStats::default() };
        for note in self.core.slots.iter().flatten() {
            stats.by_kind[note.kind.index()] += 1;
        }
        stats.entities = self.core.ents.live();
        stats.links = lock(&self.derived).graph.count();
        stats.bytes = [LOG_FILE, VECTORS_FILE, INDEX_FILE]
            .iter()
            .filter_map(|name| fs::metadata(self.dir.join(name)).ok())
            .map(|m| m.len())
            .sum();
        stats
    }

    /// The folder of the bank.
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Makes the log durable and writes `vectors.bin` and `index.bin` when they are out of
    /// date. Dropping the bank does this too, but a program that ends without running
    /// destructors (`std::process::exit`) should call it. The caches are only a speed-up:
    /// without them the next `open` rebuilds from the log.
    ///
    /// The error says which of two things failed. [`FlushError::Log`]: the notes are not known
    /// to be on disk. [`FlushError::Cache`]: the notes are safe and only the speed-up is missing.
    pub fn flush(&mut self) -> Result<(), FlushError> {
        if self.read_only {
            return Ok(());
        }
        if let Some(file) = &self.log.file {
            file.sync_data().map_err(FlushError::Log)?;
        }
        if !self.dirty && !unlock(&mut self.derived).changed {
            return Ok(());
        }
        if self.failed {
            let error = io::Error::new(io::ErrorKind::Other, "a write to notes.jsonl failed, so the caches were not written");
            return Err(FlushError::Log(error));
        }
        if fs::metadata(&self.log.path).map_or(0, |m| m.len()) != self.log.len {
            // Another bank appended since this one last looked. Caches made from this view
            // would be out of date and would replace the ones that bank wrote; the next bank
            // to write, or the next open, makes current ones.
            return Ok(());
        }
        match self.write_caches() {
            Ok(()) => {
                self.cache_reported = false;
                Ok(())
            }
            Err(error) => {
                self.cache_reported = true;
                Err(FlushError::Cache(error))
            }
        }
    }

    /// Writes `vectors.bin` and `index.bin` from the state in memory.
    fn write_caches(&mut self) -> io::Result<()> {
        let derived = unlock(&mut self.derived);
        let slots = &self.core.slots;
        let bm25 = derived.bm25.get_or_insert_with(|| build_bm25(slots));
        derived.vecs.grow(slots.len());
        derived.vecs.write(&self.dir.join(VECTORS_FILE))?;
        let parts = Parts {
            log_len: self.log.len,
            next_seq: self.core.next_seq,
            dim: derived.vecs.dim,
            slots,
            have: &derived.vecs.have,
            bm25,
            ents: &self.core.ents,
            graph: &derived.graph,
        };
        save_snapshot(&self.dir.join(INDEX_FILE), &parts)?;
        derived.changed = false;
        self.dirty = false;
        Ok(())
    }

    fn append(&mut self, lines: &[String]) -> io::Result<()> {
        let result = self.log.append(lines.concat().as_bytes());
        if result.is_err() {
            self.failed = true;
        } else {
            self.cache_reported = false;
        }
        result
    }

    /// Brings the memory up to date with `notes.jsonl` when another bank has appended to it
    /// since this one last looked, or when an earlier failure left the memory ahead of the
    /// file. The log lock must be held, so that the file cannot change again before the
    /// caller's own write.
    fn catch_up(&mut self) -> io::Result<()> {
        let on_disk = fs::metadata(&self.log.path).map_or(0, |m| m.len());
        if self.failed || on_disk != self.log.len {
            self.reload()?;
            self.failed = false;
        }
        Ok(())
    }

    /// Rebuilds the notes from `notes.jsonl`, as an open without caches does. The vectors and
    /// the semantic links are made again by the next call that passes an embedder.
    fn reload(&mut self) -> io::Result<()> {
        // The time is taken before the bytes: a write that lands in between makes the next
        // `refresh` read again instead of being missed.
        let modified = fs::metadata(&self.log.path).ok().and_then(|m| m.modified().ok());
        let bytes = match fs::read(&self.log.path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == io::ErrorKind::NotFound => Vec::new(),
            Err(error) => return Err(error),
        };
        let (core, graph) = Core::from_log(&bytes);
        let slots = core.slots.len();
        self.core = core;
        *unlock(&mut self.derived) = Derived { vecs: VecStore::empty(0, slots), graph, bm25: None, changed: false };
        self.log.len = bytes.len() as u64;
        self.log.modified = modified;
        self.log.ends_clean = bytes.last().map_or(true, |&b| b == b'\n');
        self.dirty = true;
        self.cache_reported = false;
        Ok(())
    }

    /// After a change that did not reach the file: puts the memory back to what the file
    /// says. When that is not possible the bank stays marked as failed, writes no caches, and
    /// tries again before its next change.
    fn restore(&mut self) {
        self.failed = self.reload().is_err();
    }
}

impl Drop for Bank {
    fn drop(&mut self) {
        // Nobody can be told here, so a failure is at least printed. A cache failure that an
        // earlier call of `flush` already gave to the caller is not printed again.
        let reported = self.cache_reported;
        match self.flush() {
            Ok(()) => {}
            Err(FlushError::Cache(_)) if reported => {}
            Err(error) => eprintln!("lxr-memory: could not finish writing {}: {error}", self.dir.display()),
        }
    }
}

/// Locks the derived parts; a panic elsewhere does not make them unusable.
pub(crate) fn lock(derived: &Mutex<Derived>) -> MutexGuard<'_, Derived> {
    derived.lock().unwrap_or_else(PoisonError::into_inner)
}

fn unlock(derived: &mut Mutex<Derived>) -> &mut Derived {
    derived.get_mut().unwrap_or_else(PoisonError::into_inner)
}

/// True when the file is empty or its last byte is a line break.
fn ends_with_newline(path: &Path, len: u64) -> bool {
    if len == 0 {
        return true;
    }
    let read = || -> io::Result<u8> {
        let mut file = File::open(path)?;
        file.seek(SeekFrom::Start(len - 1))?;
        let mut byte = [0u8; 1];
        file.read_exact(&mut byte)?;
        Ok(byte[0])
    };
    read().map_or(true, |b| b == b'\n')
}

/// Loads the caches if they are current: both newer than the log and made for a log of this
/// length. A missing or mismatched `vectors.bin` leaves the vectors to be made later.
fn from_snapshot(dir: &Path, log_len: u64, log_time: Option<std::time::SystemTime>) -> Option<(Core, Derived)> {
    let current = |path: &Path| {
        let time = fs::metadata(path).ok()?.modified().ok();
        Some(match (time, log_time) {
            (Some(file), Some(log)) => file >= log,
            _ => true,
        })
    };
    let index_path = dir.join(INDEX_FILE);
    if !current(&index_path)? {
        return None;
    }
    let loaded = load_snapshot(&index_path, log_len)?;
    let count = loaded.slots.len();
    let vectors_path = dir.join(VECTORS_FILE);
    let vecs = if loaded.dim > 0 && loaded.have.iter().any(|&h| h) && current(&vectors_path) == Some(true) {
        VecStore::open(&vectors_path, loaded.dim, loaded.have.clone()).unwrap_or_else(|_| VecStore::empty(loaded.dim, count))
    } else {
        VecStore::empty(loaded.dim, count)
    };
    let mut core = Core::blank();
    core.slots = loaded.slots;
    core.next_seq = loaded.next_seq;
    core.ents = loaded.ents;
    if !core.index_basic() {
        return None;
    }
    Some((core, Derived { vecs, graph: loaded.graph, bm25: Some(loaded.bm25), changed: false }))
}

/// A tag set as a string: lower case, sorted, once each. Two notes have the same tags when
/// their keys are equal.
pub(crate) fn tag_key(tags: &[String]) -> String {
    let mut all: Vec<String> = tags.iter().map(|t| t.trim().to_lowercase()).filter(|t| !t.is_empty()).collect();
    all.sort();
    all.dedup();
    all.join("\u{1f}")
}

fn clean_tags(tags: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for tag in tags {
        let tag = tag.trim();
        if !tag.is_empty() && !out.iter().any(|t| t.eq_ignore_ascii_case(tag)) {
            out.push(tag.to_string());
        }
    }
    out
}

/// Adds the tags of `more` that `tags` lacks. True when something was added.
fn merge_tags(tags: &mut Vec<String>, more: &[String]) -> bool {
    let before = tags.len();
    for tag in clean_tags(more) {
        if !tags.iter().any(|t| t.eq_ignore_ascii_case(&tag)) {
            tags.push(tag);
        }
    }
    tags.len() != before
}

/// The text with case, spacing and sentence punctuation at its ends ignored: what "the same
/// text" means. A closing symbol or bracket changes what a fact says (`3.9` and `3.9+`, `C`
/// and `C++`), so it stays.
fn normalise(text: &str) -> String {
    let lower = collapse(&text.to_lowercase());
    lower.trim_matches(|c: char| c.is_whitespace() || SENTENCE_MARKS.contains(&c)).to_string()
}

/// True for the kinds that get temporal and semantic links.
pub(crate) fn linkable(kind: Kind) -> bool {
    matches!(kind, Kind::World | Kind::Experience)
}

/// The moment a note is about: the middle of its time, else when it was mentioned.
pub(crate) fn event_time(note: &Note) -> i64 {
    match (note.occurred_start, note.occurred_end) {
        (Some(a), Some(b)) => a + (b - a) / 2,
        (Some(a), None) | (None, Some(a)) => a,
        (None, None) => note.mentioned_at,
    }
}

/// The text a note is embedded as: `TEXT (happened DATE) [entity, entity]`. The date is
/// written without an `in`; the vectors on disk were made from exactly this shape, so it must
/// not change without making them again.
pub(crate) fn embed_text(note: &Note) -> String {
    let mut text = note.text.clone();
    if let Some(start) = note.occurred_start {
        text.push_str(&format!(" (happened {})", iso_date(start)));
    }
    if !note.entities.is_empty() {
        text.push_str(&format!(" [{}]", note.entities.join(", ")));
    }
    text
}

/// The unit vector of a text, or `None` when the text has no usable token.
fn embed_one(embedder: &Embedder, text: &str) -> Option<Vec<f32>> {
    let vector = embedder.embed_many(&[text]).pop()?;
    if vector.len() == embedder.dim() && vector.iter().any(|&x| x != 0.0) {
        Some(vector)
    } else {
        None
    }
}

/// The stored vectors nearest to `query`: at most `limit` with cosine of `min` or more,
/// best first, lower slot first among equals.
pub(crate) fn nearest(vecs: &VecStore, query: &[f32], min: f32, limit: usize) -> Vec<(u32, f32)> {
    let mut hits: Vec<(u32, f32)> = Vec::new();
    vecs.scan(query, |slot, cosine| {
        if cosine >= min {
            hits.push((slot, cosine));
        }
    });
    top(hits, limit)
}

/// The best `limit` of `(slot, score)` pairs, best first, lower slot first among equals.
pub(crate) fn top(mut hits: Vec<(u32, f32)>, limit: usize) -> Vec<(u32, f32)> {
    if limit == 0 {
        return Vec::new();
    }
    let by_rank = |a: &(u32, f32), b: &(u32, f32)| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0));
    if hits.len() > limit {
        hits.select_nth_unstable_by(limit - 1, by_rank);
        hits.truncate(limit);
    }
    hits.sort_unstable_by(by_rank);
    hits
}

/// How a cue phrase relates a fact to the one before it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Cue {
    /// "because", "since", "due to": the fact explains the one before it.
    Explains,
    /// "therefore", "so that": the fact follows from the one before it.
    Follows,
}

fn causal_cue(text: &str) -> Option<Cue> {
    let lower = text.to_lowercase();
    let words: Vec<&str> = lower.split(|c: char| !(c.is_alphanumeric() || c == '\'')).filter(|w| !w.is_empty()).collect();
    for (i, word) in words.iter().enumerate() {
        let next = words.get(i + 1).copied();
        match *word {
            "because" => return Some(Cue::Explains),
            "since" if !next.is_some_and(is_time_word) => return Some(Cue::Explains),
            "due" if next == Some("to") => return Some(Cue::Explains),
            "therefore" => return Some(Cue::Follows),
            "so" if next == Some("that") => return Some(Cue::Follows),
            _ => {}
        }
    }
    None
}

/// Words after which "since" starts a time, not a reason.
fn is_time_word(word: &str) -> bool {
    word.bytes().all(|b| b.is_ascii_digit())
        || matches!(
            word,
            "then" | "last" | "yesterday" | "today" | "now" | "monday" | "tuesday" | "wednesday" | "thursday" | "friday"
                | "saturday" | "sunday" | "january" | "february" | "march" | "april" | "may" | "june" | "july" | "august"
                | "september" | "october" | "november" | "december"
        )
}

/// What one `retain` call collects while it works.
struct Batch<'a> {
    opts: &'a RetainOpts,
    now: i64,
    /// Log lines, in the order the notes first appear.
    lines: Vec<String>,
    /// `(entity, tag key)` pairs whose observation may need a refresh.
    touched: Vec<(u32, String)>,
    /// Notes whose state changed after their line was written.
    modified: Vec<u32>,
    /// The note made from the fact before this one.
    prev: Option<(String, u32)>,
}

impl Core {
    pub(crate) fn blank() -> Core {
        Core {
            slots: Vec::new(),
            by_id: HashMap::new(),
            next_seq: 1,
            ents: EntityTable::default(),
            note_ents: Vec::new(),
            times: BTreeSet::new(),
            dup: HashMap::new(),
            obs: HashMap::new(),
            lazy_ready: false,
        }
    }

    /// Rebuilds everything that can be derived from the notes alone: the entity table and the
    /// temporal and causal links. The vectors, and the semantic links with them, need an
    /// embedder and are made later.
    fn from_log(bytes: &[u8]) -> (Core, Graph) {
        let played = replay(bytes);
        let mut core = Core::blank();
        core.next_seq = played.max_seq + 1;
        core.slots = played.slots;
        for (slot, note) in core.slots.iter().enumerate() {
            let Some(note) = note else { continue };
            let ids: Vec<u32> = note.entities.iter().map(|name| core.ents.create(name, note.mentioned_at)).collect();
            let mut unique = Vec::new();
            for id in ids {
                if !unique.contains(&id) {
                    unique.push(id);
                }
            }
            core.ents.attach(&unique, slot as u32, note.mentioned_at, note.kind != Kind::Observation);
        }
        core.index_basic();
        let mut graph = Graph::default();
        graph.ensure(core.slots.len());
        let mut times: BTreeSet<(i64, u32)> = BTreeSet::new();
        for (slot, note) in core.slots.iter().enumerate() {
            let Some(note) = note else { continue };
            if linkable(note.kind) {
                let t = event_time(note);
                for (other, w) in temporal_neighbors(&times, t, TEMPORAL_LIMIT) {
                    graph.add(slot as u32, other, LinkKind::Temporal, w);
                }
                times.insert((t, slot as u32));
            }
            for cause in &note.caused_by {
                if let Some(&other) = core.by_id.get(cause) {
                    if !graph.has(slot as u32, other, LinkKind::Causal) {
                        graph.add(slot as u32, other, LinkKind::Causal, 1.0);
                    }
                }
            }
        }
        (core, graph)
    }

    /// Fills `by_id` and `note_ents` from the notes and the entity table. False when a note
    /// names an entity the table does not know.
    pub(crate) fn index_basic(&mut self) -> bool {
        self.by_id.clear();
        self.by_id.reserve(self.slots.len());
        self.note_ents = vec![Vec::new(); self.slots.len()];
        // Notes name their entities as the table spells them, so an exact lookup finds nearly all.
        let exact: HashMap<&str, u32> = self.ents.recs.iter().enumerate().map(|(i, r)| (r.name.as_str(), i as u32)).collect();
        for (slot, note) in self.slots.iter().enumerate() {
            let Some(note) = note else { continue };
            self.by_id.insert(note.id.clone(), slot as u32);
            for name in &note.entities {
                let id = match exact.get(name.as_str()) {
                    Some(&id) => id,
                    None => match self.ents.id_of(name) {
                        Some(id) => id,
                        None => return false,
                    },
                };
                if !self.note_ents[slot].contains(&id) {
                    self.note_ents[slot].push(id);
                }
            }
        }
        self.lazy_ready = false;
        true
    }

    /// Builds the indexes that only writing needs, once.
    fn ensure_indexes(&mut self) {
        if self.lazy_ready {
            return;
        }
        self.times.clear();
        self.dup.clear();
        self.obs.clear();
        for (slot, note) in self.slots.iter().enumerate() {
            let Some(note) = note else { continue };
            self.dup.insert((note.kind.index() as u8, hash64(normalise(&note.text).as_bytes())), slot as u32);
            if note.kind == Kind::Observation && !note.sources.is_empty() {
                let key = tag_key(&note.tags);
                for &entity in &self.note_ents[slot] {
                    self.obs.insert((entity, key.clone()), slot as u32);
                }
            }
            if linkable(note.kind) {
                self.times.insert((event_time(note), slot as u32));
            }
        }
        self.lazy_ready = true;
    }

    pub(crate) fn note_at(&self, slot: u32) -> Option<&Note> {
        self.slots.get(slot as usize)?.as_ref()
    }

    /// Makes the vectors fit this embedder, then gives a vector and semantic links to every
    /// note that lacks them.
    pub(crate) fn sync_embedder(&self, derived: &mut Derived, embedder: &Embedder) {
        if derived.vecs.dim != embedder.dim() {
            derived.vecs.reset(embedder.dim());
        }
        derived.vecs.grow(self.slots.len());
        let todo: Vec<u32> = self
            .slots
            .iter()
            .enumerate()
            .filter(|&(slot, note)| note.is_some() && !derived.vecs.have[slot])
            .map(|(slot, _)| slot as u32)
            .collect();
        if todo.is_empty() {
            return;
        }
        derived.changed = true;
        let texts: Vec<String> = todo.iter().filter_map(|&s| self.note_at(s)).map(embed_text).collect();
        let refs: Vec<&str> = texts.iter().map(String::as_str).collect();
        let vectors = embedder.embed_many(&refs);
        let mut linked: Vec<u32> = Vec::new();
        for (&slot, vector) in todo.iter().zip(&vectors) {
            if vector.iter().any(|&x| x != 0.0) {
                derived.vecs.set(slot, vector);
                if self.note_at(slot).is_some_and(|n| linkable(n.kind)) {
                    linked.push(slot);
                }
            }
        }
        let flags: Vec<bool> = self.slots.iter().map(|n| n.as_ref().is_some_and(|n| linkable(n.kind))).collect();
        let found = link_candidates(&derived.vecs, &flags, &linked);
        for (&slot, hits) in linked.iter().zip(found) {
            for (other, cosine) in hits {
                if !derived.graph.has(slot, other, LinkKind::Semantic) {
                    derived.graph.add(slot, other, LinkKind::Semantic, cosine);
                }
            }
        }
        derived.bm25 = None;
    }

    /// Takes one retain call through: facts, entities, anchors, times, repeats, links,
    /// observations. Returns what happened to each fact and the log lines to append.
    fn retain(
        &mut self,
        derived: &mut Derived,
        text: &str,
        opts: &RetainOpts,
        code: &dyn CodeView,
        embedder: Option<&Embedder>,
    ) -> io::Result<(Vec<Retained>, Vec<String>)> {
        let facts = if opts.split { split_facts(text) } else { whole(text).into_iter().collect() };
        if facts.is_empty() {
            return Ok((Vec::new(), Vec::new()));
        }
        self.ensure_indexes();
        if let Some(embedder) = embedder {
            self.sync_embedder(derived, embedder);
        }
        let mut batch =
            Batch { opts, now: opts.now.unwrap_or_else(unix_now), lines: Vec::new(), touched: Vec::new(), modified: Vec::new(), prev: None };
        let mut results = Vec::with_capacity(facts.len());
        for fact in &facts {
            results.push(self.retain_fact(derived, fact, &mut batch, code, embedder)?);
        }
        let mut modified = std::mem::take(&mut batch.modified);
        modified.sort_unstable();
        modified.dedup();
        for slot in modified {
            if let Some(note) = self.note_at(slot) {
                batch.lines.push(note_line(note)?);
            }
        }
        self.refresh_observations(derived, &mut batch, embedder)?;
        Ok((results, batch.lines))
    }

    fn retain_fact(
        &mut self,
        derived: &mut Derived,
        fact: &str,
        batch: &mut Batch,
        code: &dyn CodeView,
        embedder: Option<&Embedder>,
    ) -> io::Result<Retained> {
        let (opts, now) = (batch.opts, batch.now);
        let kind = opts.kind;
        let norm = normalise(fact);
        let key = (kind.index() as u8, hash64(norm.as_bytes()));
        if let Some(&slot) = self.dup.get(&key) {
            if self.note_at(slot).is_some_and(|n| n.kind == kind && normalise(&n.text) == norm) {
                let id = self.reinforce(derived, slot, batch, "same text", code);
                return self.finish_repeat(id?, slot, batch);
            }
        }

        // Entities: the caller's, then those the rules find, merged into known ones.
        let wanted: Vec<String> = opts.entities.iter().map(|e| e.trim().to_string()).filter(|e| !e.is_empty()).collect();
        let names = dedupe([wanted, extract(fact)].concat());
        let exact: Vec<Option<u32>> = names.iter().map(|n| self.ents.id_of(n)).collect();
        let mut nearby: Vec<u32> = exact.iter().flatten().copied().collect();
        let mut pairs: Vec<(String, Option<u32>)> = Vec::new();
        let mut raw: Vec<String> = Vec::new();
        for (name, exact_id) in names.iter().zip(&exact) {
            // A likeness of spelling merges a name into a known entity, unless the code knows
            // the two as definitions of their own (`get_user` and `get_users`).
            let id = exact_id.or_else(|| {
                self.ents.resolve(name, &nearby, now).filter(|&known| !self.separate_definitions(code, name, known))
            });
            if let Some(id) = id {
                if !nearby.contains(&id) {
                    nearby.push(id);
                }
            }
            let shown = id.map_or_else(|| name.clone(), |i| self.ents.recs[i as usize].name.clone());
            if pairs.iter().any(|(p, _)| fold(p) == fold(&shown)) {
                continue;
            }
            pairs.push((shown, id));
            raw.push(name.clone());
        }
        let known: Vec<u32> = pairs.iter().filter_map(|&(_, id)| id).collect();

        // Anchors: the definitions the entities name, with their hash now.
        let mut anchors: Vec<Anchor> = Vec::new();
        if code.knows_code() {
            for ((shown, _), original) in pairs.iter().zip(&raw) {
                // The name as written first: `shown` is the entity's name, which may be the
                // spelling of another definition.
                let anchor = code.anchor(original).or_else(|| code.anchor(shown));
                if let Some(anchor) = anchor {
                    if !anchors.contains(&anchor) && anchors.len() < ANCHOR_LIMIT {
                        anchors.push(anchor);
                    }
                }
            }
        }

        // Times: the caller's, else a date in the text.
        let (occurred_start, occurred_end) = match opts.when {
            Some(t) => (Some(t), Some(t)),
            None => match parse_span(fact, now, opts.utc_offset) {
                Some((a, b)) => (Some(a), Some(b)),
                None => (None, None),
            },
        };

        let mut note = Note {
            id: String::new(),
            kind,
            text: fact.to_string(),
            context: opts.context.trim().to_string(),
            tags: clean_tags(&opts.tags),
            entities: pairs.iter().map(|(name, _)| name.clone()).collect(),
            anchors,
            occurred_start,
            occurred_end,
            mentioned_at: now,
            created_at: now,
            updated_at: now,
            proof_count: 1,
            sources: Vec::new(),
            history: Vec::new(),
            caused_by: Vec::new(),
        };

        // A near repeat of an older note of the same kind with an entity in common.
        let vector = embedder.and_then(|e| embed_one(e, &embed_text(&note)));
        let neighbors = match &vector {
            Some(v) => nearest(&derived.vecs, v, LINK_THRESHOLD.min(NEAR_DUPLICATE), SEMANTIC_LIMIT + 8),
            None => Vec::new(),
        };
        let repeat = neighbors.iter().find(|&&(slot, cosine)| {
            cosine >= NEAR_DUPLICATE
                && self.note_at(slot).is_some_and(|n| n.kind == kind)
                && self.note_ents.get(slot as usize).is_some_and(|theirs| {
                    (known.is_empty() && theirs.is_empty()) || known.iter().any(|id| theirs.contains(id))
                })
        });
        if let Some(&(slot, cosine)) = repeat {
            let how = format!("near duplicate, cosine {cosine:.2}");
            let id = self.reinforce(derived, slot, batch, &how, code);
            return self.finish_repeat(id?, slot, batch);
        }

        // A new note.
        let slot = self.slots.len() as u32;
        note.id = id_for(self.next_seq);
        self.next_seq += 1;
        let mut ids: Vec<u32> = Vec::new();
        for (shown, id) in &pairs {
            let id = id.unwrap_or_else(|| self.ents.create(shown, now));
            if !ids.contains(&id) {
                ids.push(id);
            }
        }
        if let Some((prev_id, prev_slot)) = batch.prev.clone() {
            match causal_cue(fact) {
                Some(Cue::Explains) => {
                    if let Some(Some(earlier)) = self.slots.get_mut(prev_slot as usize) {
                        if !earlier.caused_by.contains(&note.id) {
                            earlier.caused_by.push(note.id.clone());
                        }
                        batch.modified.push(prev_slot);
                    }
                    derived.graph.add(prev_slot, slot, LinkKind::Causal, 1.0);
                }
                Some(Cue::Follows) => {
                    note.caused_by.push(prev_id);
                    derived.graph.add(prev_slot, slot, LinkKind::Causal, 1.0);
                }
                None => {}
            }
        }
        let tags_key = tag_key(&note.tags);
        let time = event_time(&note);
        let id = note.id.clone();
        let line = note_line(&note)?;
        self.push_note(derived, note, ids.clone(), vector);
        batch.lines.push(line);
        if linkable(kind) {
            for (other, w) in temporal_neighbors(&self.times, time, TEMPORAL_LIMIT) {
                derived.graph.add(slot, other, LinkKind::Temporal, w);
            }
            self.times.insert((time, slot));
            let mut made = 0;
            for &(other, cosine) in &neighbors {
                if made == SEMANTIC_LIMIT {
                    break;
                }
                if cosine >= LINK_THRESHOLD && other != slot && self.note_at(other).is_some_and(|n| linkable(n.kind)) {
                    derived.graph.add(slot, other, LinkKind::Semantic, cosine);
                    made += 1;
                }
            }
        }
        if matches!(kind, Kind::World | Kind::Experience) {
            batch.touched.extend(ids.iter().map(|&e| (e, tags_key.clone())));
        }
        batch.prev = Some((id.clone(), slot));
        Ok(Retained { id, outcome: RetainOutcome::Added })
    }

    fn finish_repeat(&mut self, id: String, slot: u32, batch: &mut Batch) -> io::Result<Retained> {
        batch.prev = Some((id.clone(), slot));
        Ok(Retained { id, outcome: RetainOutcome::Reinforced })
    }

    /// True when the code view knows `name` and the entity `known` as two definitions of their
    /// own, so that a likeness of spelling must not merge them.
    fn separate_definitions(&self, code: &dyn CodeView, name: &str, known: u32) -> bool {
        if !code.knows_code() {
            return false;
        }
        match (code.anchor(name), code.anchor(&self.ents.recs[known as usize].name)) {
            (Some(a), Some(b)) => a != b,
            _ => false,
        }
    }

    /// Raises a note's proof count and records why. Returns its id. A repeat is retained against
    /// the code as it is now, so the hash of every definition the note still talks about is
    /// brought up to date: the note stops being stale. A definition that is gone stays gone.
    fn reinforce(&mut self, derived: &mut Derived, slot: u32, batch: &mut Batch, how: &str, code: &dyn CodeView) -> io::Result<String> {
        let Some(Some(note)) = self.slots.get_mut(slot as usize) else {
            return Err(io::Error::new(io::ErrorKind::NotFound, "the note to reinforce is gone"));
        };
        let before = tag_key(&note.tags);
        note.proof_count = note.proof_count.saturating_add(1);
        note.updated_at = batch.now;
        if code.knows_code() {
            for anchor in &mut note.anchors {
                if let Some(hash) = code.current_hash(anchor) {
                    anchor.hash = hash;
                }
            }
        }
        note.history.push(format!("{}: seen again ({how})", iso_date(batch.now)));
        if note.history.len() > HISTORY_LIMIT {
            note.history.remove(0);
        }
        let changed = merge_tags(&mut note.tags, &batch.opts.tags);
        let after = tag_key(&note.tags);
        let id = note.id.clone();
        batch.lines.push(note_line(note)?);
        if changed {
            derived.bm25 = None;
            if matches!(note.kind, Kind::World | Kind::Experience) {
                for &entity in &self.note_ents[slot as usize] {
                    batch.touched.push((entity, before.clone()));
                    batch.touched.push((entity, after.clone()));
                }
            }
        }
        Ok(id)
    }

    /// Stores a note in the next slot and in every index.
    fn push_note(&mut self, derived: &mut Derived, note: Note, ids: Vec<u32>, vector: Option<Vec<f32>>) -> u32 {
        let slot = self.slots.len() as u32;
        self.ents.attach(&ids, slot, note.mentioned_at, note.kind != Kind::Observation);
        self.by_id.insert(note.id.clone(), slot);
        self.dup.insert((note.kind.index() as u8, hash64(normalise(&note.text).as_bytes())), slot);
        if note.kind == Kind::Observation && !note.sources.is_empty() {
            let key = tag_key(&note.tags);
            for &entity in &ids {
                self.obs.insert((entity, key.clone()), slot);
            }
        }
        self.note_ents.push(ids);
        self.slots.push(Some(note));
        derived.vecs.grow(self.slots.len());
        derived.graph.ensure(self.slots.len());
        if let Some(vector) = vector {
            derived.vecs.set(slot, &vector);
        }
        derived.bm25 = None;
        slot
    }

    /// Takes a note out of every index. Its slot stays empty.
    fn drop_note(&mut self, derived: &mut Derived, slot: u32) {
        let Some(note) = self.slots.get_mut(slot as usize).and_then(Option::take) else {
            return;
        };
        self.by_id.remove(&note.id);
        let ids = std::mem::take(&mut self.note_ents[slot as usize]);
        self.ents.detach(&ids, slot, note.kind != Kind::Observation);
        let dup_key = (note.kind.index() as u8, hash64(normalise(&note.text).as_bytes()));
        if self.dup.get(&dup_key) == Some(&slot) {
            self.dup.remove(&dup_key);
        }
        if note.kind == Kind::Observation {
            self.obs.retain(|_, &mut s| s != slot);
        }
        self.times.remove(&(event_time(&note), slot));
        derived.vecs.clear(slot);
        derived.graph.remove_node(slot);
        derived.bm25 = None;
    }

    /// Forgets a note and returns the log lines, or `None` when there is no such note.
    fn forget(&mut self, derived: &mut Derived, id: &str, now: i64) -> io::Result<Option<Vec<String>>> {
        let Some(&slot) = self.by_id.get(id) else {
            return Ok(None);
        };
        self.ensure_indexes();
        let (kind, tags_key, ids) = match self.note_at(slot) {
            Some(n) => (n.kind, tag_key(&n.tags), self.note_ents[slot as usize].clone()),
            None => return Ok(None),
        };
        self.drop_note(derived, slot);
        let opts = RetainOpts::default();
        let mut batch = Batch { opts: &opts, now, lines: vec![forget_line(id, now)?], touched: Vec::new(), modified: Vec::new(), prev: None };
        if matches!(kind, Kind::World | Kind::Experience) {
            batch.touched.extend(ids.into_iter().map(|e| (e, tags_key.clone())));
        }
        self.refresh_observations(derived, &mut batch, None)?;
        Ok(Some(batch.lines))
    }

    /// The notes (slots, oldest first) that an observation of `entity` with the tag set
    /// `tags_key` summarises: the ordinary notes about the entity that carry exactly those tags.
    fn group_of(&self, entity: u32, tags_key: &str) -> Vec<u32> {
        self.ents.recs[entity as usize]
            .notes
            .iter()
            .copied()
            .filter(|&slot| {
                self.note_at(slot).is_some_and(|n| {
                    matches!(n.kind, Kind::World | Kind::Experience)
                        // An untagged note has the empty key and belongs to the untagged group.
                        && if n.tags.is_empty() { tags_key.is_empty() } else { tag_key(&n.tags) == tags_key }
                })
            })
            .collect()
    }

    /// Brings the observations in line with the notes: for each touched `(entity, tags)`, three
    /// or more notes that share them have an observation that lists them as sources; fewer have
    /// none. Entities that are about exactly the same notes share one observation, which lists
    /// all of them as its subjects, instead of each getting a copy of the same facts.
    ///
    /// A bank written before that rule holds one copy of an observation per entity. The next write
    /// of any kind settles those too, so that such a bank heals itself without a read changing it.
    fn refresh_observations(&mut self, derived: &mut Derived, batch: &mut Batch, embedder: Option<&Embedder>) -> io::Result<()> {
        let mut touched = std::mem::take(&mut batch.touched);
        touched.extend(self.observation_copies());
        let mut by_tags: BTreeMap<String, BTreeSet<u32>> = BTreeMap::new();
        for (entity, tags_key) in touched {
            by_tags.entry(tags_key).or_default().insert(entity);
        }
        for (tags_key, entities) in by_tags {
            self.reconcile(derived, batch, embedder, &tags_key, entities)?;
        }
        Ok(())
    }

    /// The `(entity, tags)` pairs of observations that are copies of each other: the same tag set
    /// and the same sources, which is what an observation per entity looks like when the entities
    /// are about the same notes.
    fn observation_copies(&self) -> Vec<(u32, String)> {
        let mut by_content: HashMap<(String, &[String]), Vec<u32>> = HashMap::new();
        for (slot, note) in self.slots.iter().enumerate() {
            if let Some(note) = note.as_ref().filter(|n| n.kind == Kind::Observation && !n.sources.is_empty()) {
                by_content.entry((tag_key(&note.tags), note.sources.as_slice())).or_default().push(slot as u32);
            }
        }
        let mut pairs = Vec::new();
        for ((tags_key, _), slots) in by_content {
            if slots.len() > 1 {
                for slot in slots {
                    pairs.extend(self.note_ents[slot as usize].iter().map(|&entity| (entity, tags_key.clone())));
                }
            }
        }
        pairs
    }

    /// Settles the observations of one tag set. `entities` are the entities whose notes changed.
    fn reconcile(
        &mut self,
        derived: &mut Derived,
        batch: &mut Batch,
        embedder: Option<&Embedder>,
        tags_key: &str,
        mut entities: BTreeSet<u32>,
    ) -> io::Result<()> {
        // The entities to look at: the ones that changed, the others that an observation of
        // theirs lists as a subject, and the others that are about the same notes as one of
        // them (an observation made before this rule has one copy per entity, and a note that
        // is forgotten can make two groups the same again).
        let mut groups: BTreeMap<u32, Vec<u32>> = BTreeMap::new();
        let mut pending: Vec<u32> = entities.iter().copied().collect();
        while let Some(entity) = pending.pop() {
            let group = self.group_of(entity, tags_key);
            let mut joining: Vec<u32> = Vec::new();
            if let Some(&slot) = self.obs.get(&(entity, tags_key.to_string())) {
                joining.extend(self.note_ents[slot as usize].iter().copied());
            }
            if let Some(&first) = group.first().filter(|_| group.len() >= OBSERVATION_FROM) {
                for &other in &self.note_ents[first as usize] {
                    if !entities.contains(&other) && self.group_of(other, tags_key) == group {
                        joining.push(other);
                    }
                }
            }
            groups.insert(entity, group);
            for other in joining {
                if entities.insert(other) {
                    pending.push(other);
                }
            }
        }

        // The observations there should be: one per set of notes of three or more, with the
        // entities that are about exactly those notes as its subjects, in the order the oldest
        // note names them.
        let mut classes: BTreeMap<Vec<u32>, Vec<u32>> = BTreeMap::new();
        for (&entity, group) in &groups {
            if group.len() >= OBSERVATION_FROM {
                classes.entry(group.clone()).or_default().push(entity);
            }
        }
        let mut wanted: Vec<(Vec<u32>, Vec<u32>)> = Vec::new();
        for (members, class) in classes {
            let mut subjects: Vec<u32> = self.note_ents[members[0] as usize].iter().copied().filter(|e| class.contains(e)).collect();
            let missing: Vec<u32> = class.iter().copied().filter(|e| !subjects.contains(e)).collect();
            subjects.extend(missing);
            wanted.push((members, subjects));
        }

        // The observations there are, for those entities.
        let existing: BTreeSet<u32> = entities.iter().filter_map(|&e| self.obs.get(&(e, tags_key.to_string())).copied()).collect();

        let mut kept: BTreeSet<u32> = BTreeSet::new();
        for (members, subjects) in wanted {
            let keeper = existing
                .iter()
                .copied()
                .find(|slot| !kept.contains(slot) && same_set(&self.note_ents[*slot as usize], &subjects));
            match keeper {
                Some(slot) => {
                    kept.insert(slot);
                    self.update_observation(derived, batch, embedder, slot, &members)?;
                }
                None => self.make_observation(derived, batch, embedder, &members, &subjects)?,
            }
        }
        for &slot in existing.difference(&kept) {
            let id = self.note_at(slot).map(|n| n.id.clone());
            self.drop_note(derived, slot);
            if let Some(id) = id {
                batch.lines.push(forget_line(&id, batch.now)?);
            }
        }
        Ok(())
    }

    /// What an observation says about `members`: the facts, newest last, cut to size.
    fn summarise(&self, members: &[u32], names: &[String]) -> Summary {
        let notes: Vec<&Note> = members.iter().filter_map(|&s| self.note_at(s)).collect();
        let count = notes.len();
        let sources = notes[count.saturating_sub(SOURCE_LIMIT)..].iter().map(|n| n.id.clone()).collect();
        // The first characters of the names and the texts: the members are not read past the cut.
        let mut text = format!("{}:", names.join(", "));
        let mut chars = text.chars().count();
        for member in &notes {
            if chars >= OBSERVATION_CHARS {
                break;
            }
            text.push(' ');
            text.push_str(&member.text);
            chars += 1 + member.text.chars().count();
        }
        let text: String = text.chars().take(OBSERVATION_CHARS).collect();
        let mut anchors: Vec<Anchor> = Vec::new();
        for anchor in notes.iter().flat_map(|n| n.anchors.iter()) {
            if anchors.len() >= 2 * ANCHOR_LIMIT {
                break;
            }
            if !anchors.contains(anchor) {
                anchors.push(anchor.clone());
            }
        }
        Summary {
            text,
            sources,
            anchors,
            occurred_start: notes.iter().filter_map(|n| n.occurred_start).min(),
            occurred_end: notes.iter().filter_map(|n| n.occurred_end).max(),
            count,
            tags: notes.first().map(|n| n.tags.clone()).unwrap_or_default(),
        }
    }

    /// Refreshes the observation in `slot` from its members, unless it already says the same.
    fn update_observation(
        &mut self,
        derived: &mut Derived,
        batch: &mut Batch,
        embedder: Option<&Embedder>,
        slot: u32,
        members: &[u32],
    ) -> io::Result<()> {
        let Some(names) = self.note_at(slot).map(|n| n.entities.clone()) else { return Ok(()) };
        let summary = self.summarise(members, &names);
        if self.note_at(slot).is_some_and(|n| n.sources == summary.sources && n.proof_count as usize == summary.count) {
            return Ok(());
        }
        let Some(Some(note)) = self.slots.get_mut(slot as usize) else { return Ok(()) };
        note.text = summary.text;
        note.sources = summary.sources;
        note.anchors = summary.anchors;
        note.occurred_start = summary.occurred_start;
        note.occurred_end = summary.occurred_end;
        note.proof_count = summary.count as u32;
        note.updated_at = batch.now;
        note.history.push(format!("{}: refreshed from {} notes", iso_date(batch.now), summary.count));
        if note.history.len() > HISTORY_LIMIT {
            note.history.remove(0);
        }
        batch.lines.push(note_line(note)?);
        let vector = embedder.and_then(|e| embed_one(e, &embed_text(note)));
        match vector {
            Some(v) => derived.vecs.set(slot, &v),
            None => derived.vecs.clear(slot),
        }
        derived.bm25 = None;
        Ok(())
    }

    /// Makes a new observation of `members` about the entities `subjects`.
    fn make_observation(
        &mut self,
        derived: &mut Derived,
        batch: &mut Batch,
        embedder: Option<&Embedder>,
        members: &[u32],
        subjects: &[u32],
    ) -> io::Result<()> {
        let names: Vec<String> = subjects.iter().map(|&e| self.ents.recs[e as usize].name.clone()).collect();
        let summary = self.summarise(members, &names);
        let count = summary.count;
        let note = Note {
            id: id_for(self.next_seq),
            kind: Kind::Observation,
            text: summary.text,
            context: format!("consolidated from {count} notes"),
            tags: summary.tags,
            entities: names,
            anchors: summary.anchors,
            occurred_start: summary.occurred_start,
            occurred_end: summary.occurred_end,
            mentioned_at: batch.now,
            created_at: batch.now,
            updated_at: batch.now,
            proof_count: count as u32,
            sources: summary.sources,
            history: vec![format!("{}: built from {count} notes", iso_date(batch.now))],
            caused_by: Vec::new(),
        };
        self.next_seq += 1;
        let vector = embedder.and_then(|e| embed_one(e, &embed_text(&note)));
        batch.lines.push(note_line(&note)?);
        self.push_note(derived, note, subjects.to_vec(), vector);
        Ok(())
    }
}

/// What an observation says about its members.
struct Summary {
    text: String,
    /// The ids of the newest members, at most [`SOURCE_LIMIT`].
    sources: Vec<String>,
    anchors: Vec<Anchor>,
    occurred_start: Option<i64>,
    occurred_end: Option<i64>,
    /// The number of members, whole although `sources` is capped.
    count: usize,
    tags: Vec<String>,
}

/// True when the two lists hold the same ids, in any order.
fn same_set(a: &[u32], b: &[u32]) -> bool {
    let (mut a, mut b) = (a.to_vec(), b.to_vec());
    a.sort_unstable();
    b.sort_unstable();
    a == b
}

/// For each of `targets`, the nearest linkable notes at or above the link threshold, at most
/// 50 each. Large batches are spread over the available cores.
fn link_candidates(vecs: &VecStore, linkable_slots: &[bool], targets: &[u32]) -> Vec<Vec<(u32, f32)>> {
    let work = |slot: u32| -> Vec<(u32, f32)> {
        let Some(query) = vecs.row(slot) else {
            return Vec::new();
        };
        let mut hits = Vec::new();
        vecs.scan(query, |other, cosine| {
            if other != slot && cosine >= LINK_THRESHOLD && linkable_slots.get(other as usize).copied().unwrap_or(false) {
                hits.push((other, cosine));
            }
        });
        top(hits, SEMANTIC_LIMIT)
    };
    if targets.len() < 32 {
        return targets.iter().map(|&s| work(s)).collect();
    }
    let threads = std::thread::available_parallelism().map_or(1, |n| n.get()).clamp(1, 16);
    let chunk = targets.len().div_ceil(threads);
    let work = &work;
    std::thread::scope(|scope| {
        let handles: Vec<_> = targets.chunks(chunk).map(|part| scope.spawn(move || part.iter().map(|&s| work(s)).collect::<Vec<_>>())).collect();
        handles.into_iter().flat_map(|h| h.join().unwrap_or_default()).collect()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tag_keys_ignore_case_order_and_repeats() {
        let a = tag_key(&["B".to_string(), "a".to_string(), "A".to_string()]);
        let b = tag_key(&["a".to_string(), "b".to_string()]);
        assert_eq!(a, b);
        assert_ne!(a, tag_key(&["a".to_string()]));
        assert_eq!(tag_key(&[]), "");
    }

    #[test]
    fn tags_are_cleaned_and_merged() {
        assert_eq!(clean_tags(&[" x ".into(), "X".into(), "".into(), "y".into()]), vec!["x", "y"]);
        let mut tags = vec!["a".to_string()];
        assert!(merge_tags(&mut tags, &["A".to_string(), "b".to_string()]));
        assert_eq!(tags, vec!["a", "b"]);
        assert!(!merge_tags(&mut tags, &["B".to_string()]));
    }

    #[test]
    fn normalised_text_ignores_case_spacing_and_outer_punctuation() {
        assert_eq!(normalise("  The Cache,  is  FAST. "), normalise("the cache, is fast"));
        assert_ne!(normalise("the cache is fast"), normalise("the cache is slow"));
    }

    #[test]
    fn normalised_text_keeps_a_closing_symbol_or_bracket() {
        assert_ne!(normalise("Requires Python 3.9"), normalise("Requires Python 3.9+"));
        assert_ne!(normalise("We use C"), normalise("We use C++"));
        assert_ne!(normalise("Target is C"), normalise("Target is C#"));
        assert_ne!(normalise("Limit is 5"), normalise("Limit is 5%"));
        assert_ne!(normalise("the loader"), normalise("(the loader)"));
        assert_eq!(normalise("Wait..."), normalise("wait"));
        assert_eq!(normalise("Is it fast?!"), normalise("is it fast"));
    }

    #[test]
    fn cue_phrases_are_found_with_their_direction() {
        assert_eq!(causal_cue("It failed because the lock was held"), Some(Cue::Explains));
        assert_eq!(causal_cue("We retried, since the first call timed out"), Some(Cue::Explains));
        assert_eq!(causal_cue("This happened due to a stale cache"), Some(Cue::Explains));
        assert_eq!(causal_cue("Therefore we added a retry"), Some(Cue::Follows));
        assert_eq!(causal_cue("We batch writes so that the disk is hit once"), Some(Cue::Follows));
        assert_eq!(causal_cue("It has worked since March"), None);
        assert_eq!(causal_cue("It has worked since 2024"), None);
        assert_eq!(causal_cue("The cache is fast"), None);
        assert_eq!(causal_cue("so we left"), None);
    }

    #[test]
    fn event_time_is_the_middle_of_the_interval() {
        let mut note = Note { mentioned_at: 500, ..Note::default() };
        assert_eq!(event_time(&note), 500);
        note.occurred_start = Some(100);
        assert_eq!(event_time(&note), 100);
        note.occurred_end = Some(300);
        assert_eq!(event_time(&note), 200);
    }

    #[test]
    fn embed_text_writes_the_date_and_the_entities_after_the_text() {
        let mut note = Note { text: "the loader failed".into(), entities: vec!["load_config".into(), "src/lib.rs".into()], ..Note::default() };
        assert_eq!(embed_text(&note), "the loader failed [load_config, src/lib.rs]");
        note.occurred_start = Some(1_772_668_800);
        assert_eq!(embed_text(&note), "the loader failed (happened 2026-03-05) [load_config, src/lib.rs]");
        note.entities.clear();
        assert_eq!(embed_text(&note), "the loader failed (happened 2026-03-05)");
    }

    /// A writer that takes `room` bytes and then fails, as a full disk does.
    struct Torn {
        room: usize,
        taken: Vec<u8>,
    }

    impl Write for Torn {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            if self.room == 0 {
                return Err(io::Error::new(io::ErrorKind::Other, "disk full"));
            }
            let n = buf.len().min(self.room);
            self.taken.extend_from_slice(&buf[..n]);
            self.room -= n;
            Ok(n)
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn a_block_after_a_failed_write_starts_on_a_fresh_line() {
        let line = |id: &str| note_line(&Note { id: id.to_string(), text: "x".to_string(), ..Note::default() }).unwrap();
        let mut log = LogFile { path: PathBuf::new(), file: None, len: 0, modified: None, ends_clean: true };
        let mut torn = Torn { room: 9, taken: Vec::new() };
        assert!(log.write_block(&mut torn, line("n000001").as_bytes()).is_err());
        assert_eq!(torn.taken.len(), 9, "part of the block is on disk");
        let mut next: Vec<u8> = Vec::new();
        log.write_block(&mut next, line("n000002").as_bytes()).unwrap();
        // What the disk holds now: the fragment, then the second block.
        let mut disk = torn.taken.clone();
        disk.extend_from_slice(&next);
        let played = replay(&disk);
        assert_eq!(played.slots.len(), 1, "the second note was glued to the fragment: {:?}", String::from_utf8_lossy(&disk));
        assert_eq!(played.slots[0].as_ref().unwrap().id, "n000002");
    }

    #[test]
    fn top_orders_by_score_then_lower_slot() {
        let hits = vec![(5, 0.5), (1, 0.9), (3, 0.5), (2, 0.9), (4, 0.1)];
        assert_eq!(top(hits.clone(), 3), vec![(1, 0.9), (2, 0.9), (3, 0.5)]);
        assert_eq!(top(hits, 10).len(), 5);
        assert!(top(vec![(1, 1.0)], 0).is_empty());
    }

    fn scratch(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("lxr-memory-bank-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn a_bank_that_failed_to_write_its_log_reports_a_log_error_and_writes_no_cache() {
        let dir = scratch("flush-log");
        let mut bank = Bank::open(&dir).unwrap();
        bank.retain("the scheduler retries failed jobs", &RetainOpts::default(), &crate::NoCode, None).unwrap();
        // The state a failed append leaves behind: the memory is ahead of the file.
        bank.failed = true;
        let error = bank.flush().unwrap_err();
        assert!(error.is_log() && matches!(error, FlushError::Log(_)), "{error}");
        assert!(error.to_string().contains("notes"), "{error}");
        assert!(!dir.join(INDEX_FILE).exists(), "no cache is written from a state the file does not have");
        // The next change catches up with the file again.
        bank.failed = false;
        bank.flush().unwrap();
        assert!(dir.join(INDEX_FILE).is_file());
        drop(bank);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_cache_error_is_reported_once_and_forgotten_after_the_next_change() {
        let dir = scratch("flush-reported");
        let mut bank = Bank::open(&dir).unwrap();
        fs::create_dir_all(dir.join(INDEX_FILE)).unwrap();
        bank.retain("the scheduler retries failed jobs", &RetainOpts::default(), &crate::NoCode, None).unwrap();
        assert!(!bank.cache_reported);
        assert!(matches!(bank.flush(), Err(FlushError::Cache(_))));
        assert!(bank.cache_reported, "the caller was told, so dropping the bank stays quiet");
        bank.retain("the renderer paints widgets", &RetainOpts::default(), &crate::NoCode, None).unwrap();
        assert!(!bank.cache_reported, "a new change has not been reported yet");
        fs::remove_dir(dir.join(INDEX_FILE)).unwrap();
        bank.flush().unwrap();
        assert!(!bank.cache_reported);
        drop(bank);
        let _ = fs::remove_dir_all(&dir);
    }
}
