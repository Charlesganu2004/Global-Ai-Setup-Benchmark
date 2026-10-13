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
use std::collections::{BTreeSet, HashMap};
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, PoisonError};

/// Notes of one observation: it is built when this many notes share an entity and tags.
const OBSERVATION_FROM: usize = 3;
/// An observation's text is cut to this many characters.
const OBSERVATION_CHARS: usize = 600;
/// Most lines kept in a note's history.
const HISTORY_LIMIT: usize = 50;
/// Most anchors kept on a note.
const ANCHOR_LIMIT: usize = 8;

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
    /// `(entity, tag key)` to the slot of the observation for that pair. Built on first use.
    obs: HashMap<(u32, String), u32>,
    lazy_ready: bool,
}

/// The log file and what is known about its end.
struct LogFile {
    path: PathBuf,
    file: Option<File>,
    /// Bytes in the file, as far as this bank knows.
    len: u64,
    /// True when the file is empty or ends in a line break.
    ends_clean: bool,
}

impl LogFile {
    fn append(&mut self, bytes: &[u8]) -> io::Result<()> {
        if self.file.is_none() {
            self.file = Some(OpenOptions::new().create(true).append(true).open(&self.path)?);
        }
        let Some(file) = self.file.as_mut() else {
            return Err(io::Error::new(io::ErrorKind::Other, "the log file is not open"));
        };
        let mut block = Vec::with_capacity(bytes.len() + 1);
        if !self.ends_clean {
            block.push(b'\n');
        }
        block.extend_from_slice(bytes);
        file.write_all(&block)?;
        file.flush()?;
        self.len += block.len() as u64;
        self.ends_clean = true;
        Ok(())
    }
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
        let log_path = dir.join(LOG_FILE);
        let meta = fs::metadata(&log_path).ok();
        let log_len = meta.as_ref().map_or(0, |m| m.len());
        let log_time = meta.and_then(|m| m.modified().ok());
        let log = LogFile { ends_clean: ends_with_newline(&log_path, log_len), path: log_path, file: None, len: log_len };
        let (core, derived, dirty) = if log_len == 0 {
            (Core::blank(), Derived { vecs: VecStore::default(), graph: Graph::default(), bm25: None, changed: false }, false)
        } else if let Some((core, derived)) = from_snapshot(dir, log_len, log_time) {
            (core, derived, false)
        } else {
            let bytes = fs::read(&log.path)?;
            let (core, graph) = Core::from_log(&bytes);
            let slots = core.slots.len();
            (core, Derived { vecs: VecStore::empty(0, slots), graph, bm25: None, changed: false }, true)
        };
        Ok(Bank { dir: dir.to_path_buf(), core, derived: Mutex::new(derived), log, dirty, failed: false })
    }

    /// Remembers the facts in `text`.
    ///
    /// The text is cut into facts (unless `opts.split` is off); each fact becomes a note, or
    /// reinforces a note the bank already holds. The notes are appended to `notes.jsonl`
    /// before this returns. The caches are written by [`Bank::flush`] and when the bank is
    /// dropped.
    pub fn retain(
        &mut self,
        text: &str,
        opts: &RetainOpts,
        code: &dyn CodeView,
        embedder: Option<&Embedder>,
    ) -> io::Result<Vec<Retained>> {
        let derived = unlock(&mut self.derived);
        let (out, lines) = self.core.retain(derived, text, opts, code, embedder)?;
        if !lines.is_empty() {
            self.dirty = true;
            self.append(&lines)?;
        }
        Ok(out)
    }

    /// Recalls the notes that best answer `query`: meaning, words, links and time, fused,
    /// boosted by recency, time and proof count, and demoted when the code a note talks about
    /// has changed. Directives that pass the filters come first.
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
    /// there is no such note.
    pub fn forget(&mut self, id: &str) -> io::Result<bool> {
        let derived = unlock(&mut self.derived);
        let Some(lines) = self.core.forget(derived, id, unix_now())? else {
            return Ok(false);
        };
        self.dirty = true;
        self.append(&lines)?;
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
    pub fn flush(&mut self) -> io::Result<()> {
        if let Some(file) = &self.log.file {
            file.sync_data()?;
        }
        let derived = unlock(&mut self.derived);
        if !self.dirty && !derived.changed {
            return Ok(());
        }
        if self.failed {
            return Err(io::Error::new(io::ErrorKind::Other, "a write to notes.jsonl failed, so the caches were not written"));
        }
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
        }
        result
    }
}

impl Drop for Bank {
    fn drop(&mut self) {
        let _ = self.flush();
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

/// The text with case, spacing and outer punctuation ignored: what "the same text" means.
fn normalise(text: &str) -> String {
    let lower = collapse(&text.to_lowercase());
    lower.trim_matches(|c: char| !c.is_alphanumeric()).to_string()
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

/// The text a note is embedded as: `TEXT (happened DATE) [entity, entity]`.
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
            if note.kind == Kind::Observation && !note.sources.is_empty() && self.note_ents[slot].len() == 1 {
                self.obs.insert((self.note_ents[slot][0], tag_key(&note.tags)), slot as u32);
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
                let id = self.reinforce(derived, slot, batch, "same text");
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
            let id = exact_id.or_else(|| self.ents.resolve(name, &nearby, now));
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
                let anchor = code.anchor(shown).or_else(|| code.anchor(original));
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
            None => match parse_span(fact, now) {
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
            let id = self.reinforce(derived, slot, batch, &how);
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

    /// Raises a note's proof count and records why. Returns its id.
    fn reinforce(&mut self, derived: &mut Derived, slot: u32, batch: &mut Batch, how: &str) -> io::Result<String> {
        let Some(Some(note)) = self.slots.get_mut(slot as usize) else {
            return Err(io::Error::new(io::ErrorKind::NotFound, "the note to reinforce is gone"));
        };
        let before = tag_key(&note.tags);
        note.proof_count = note.proof_count.saturating_add(1);
        note.updated_at = batch.now;
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
        if note.kind == Kind::Observation && !note.sources.is_empty() && ids.len() == 1 {
            self.obs.insert((ids[0], tag_key(&note.tags)), slot);
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

    /// Brings the observations in line with the notes: for each touched `(entity, tags)`,
    /// three or more notes that share them have one observation that lists them as sources;
    /// fewer have none.
    fn refresh_observations(&mut self, derived: &mut Derived, batch: &mut Batch, embedder: Option<&Embedder>) -> io::Result<()> {
        let mut touched = std::mem::take(&mut batch.touched);
        touched.sort();
        touched.dedup();
        for (entity, tags_key) in touched {
            let group: Vec<u32> = self.ents.recs[entity as usize]
                .notes
                .iter()
                .copied()
                .filter(|&s| self.note_at(s).is_some_and(|n| matches!(n.kind, Kind::World | Kind::Experience) && tag_key(&n.tags) == tags_key))
                .collect();
            let existing = self.obs.get(&(entity, tags_key.clone())).copied();
            if group.len() < OBSERVATION_FROM {
                if let Some(slot) = existing {
                    let id = self.note_at(slot).map(|n| n.id.clone());
                    self.drop_note(derived, slot);
                    if let Some(id) = id {
                        batch.lines.push(forget_line(&id, batch.now)?);
                    }
                }
                continue;
            }
            let members: Vec<&Note> = group.iter().filter_map(|&s| self.note_at(s)).collect();
            let sources: Vec<String> = members.iter().map(|n| n.id.clone()).collect();
            if existing.and_then(|s| self.note_at(s)).is_some_and(|n| n.sources == sources) {
                continue;
            }
            let name = self.ents.recs[entity as usize].name.clone();
            let joined = members.iter().map(|n| n.text.as_str()).collect::<Vec<_>>().join(" ");
            let text: String = format!("{name}: {joined}").chars().take(OBSERVATION_CHARS).collect();
            let mut anchors: Vec<Anchor> = Vec::new();
            for anchor in members.iter().flat_map(|n| n.anchors.iter()) {
                if !anchors.contains(anchor) && anchors.len() < 2 * ANCHOR_LIMIT {
                    anchors.push(anchor.clone());
                }
            }
            let occurred_start = members.iter().filter_map(|n| n.occurred_start).min();
            let occurred_end = members.iter().filter_map(|n| n.occurred_end).max();
            let tags = members.first().map(|n| n.tags.clone()).unwrap_or_default();
            let count = sources.len();
            match existing {
                Some(slot) => {
                    let Some(Some(note)) = self.slots.get_mut(slot as usize) else { continue };
                    note.text = text;
                    note.sources = sources;
                    note.anchors = anchors;
                    note.occurred_start = occurred_start;
                    note.occurred_end = occurred_end;
                    note.proof_count = count as u32;
                    note.updated_at = batch.now;
                    note.history.push(format!("{}: refreshed from {count} notes", iso_date(batch.now)));
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
                }
                None => {
                    let note = Note {
                        id: id_for(self.next_seq),
                        kind: Kind::Observation,
                        text,
                        context: format!("consolidated from {count} notes"),
                        tags,
                        entities: vec![name],
                        anchors,
                        occurred_start,
                        occurred_end,
                        mentioned_at: batch.now,
                        created_at: batch.now,
                        updated_at: batch.now,
                        proof_count: count as u32,
                        sources,
                        history: vec![format!("{}: built from {count} notes", iso_date(batch.now))],
                        caused_by: Vec::new(),
                    };
                    self.next_seq += 1;
                    let vector = embedder.and_then(|e| embed_one(e, &embed_text(&note)));
                    batch.lines.push(note_line(&note)?);
                    self.push_note(derived, note, vec![entity], vector);
                }
            }
        }
        Ok(())
    }
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
    fn embed_text_follows_the_upstream_shape() {
        let mut note = Note { text: "the loader failed".into(), entities: vec!["load_config".into(), "src/lib.rs".into()], ..Note::default() };
        assert_eq!(embed_text(&note), "the loader failed [load_config, src/lib.rs]");
        note.occurred_start = Some(1_772_668_800);
        assert_eq!(embed_text(&note), "the loader failed (happened 2026-03-05) [load_config, src/lib.rs]");
        note.entities.clear();
        assert_eq!(embed_text(&note), "the loader failed (happened 2026-03-05)");
    }

    #[test]
    fn top_orders_by_score_then_lower_slot() {
        let hits = vec![(5, 0.5), (1, 0.9), (3, 0.5), (2, 0.9), (4, 0.1)];
        assert_eq!(top(hits.clone(), 3), vec![(1, 0.9), (2, 0.9), (3, 0.5)]);
        assert_eq!(top(hits, 10).len(), 5);
        assert!(top(vec![(1, 1.0)], 0).is_empty());
    }
}
