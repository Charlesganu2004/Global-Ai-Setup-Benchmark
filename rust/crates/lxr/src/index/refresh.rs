//! Making the index current.
//!
//! One walk with sizes and times tells what changed. A file is read and hashed only
//! when its size is the same and its time differs. When nothing changed, nothing else
//! is done. Otherwise the changed and new files are extracted (in parallel when there
//! are many), the stored facts of the others are reused, and the graph and the search
//! index are built again, under a lock.
//!
//! The new pieces are written beside the live ones first, so a build that fails before
//! it publishes anything leaves the old index whole. Publishing puts them in place one
//! after the other, the manifest last; the manifest holds the size and hash of the graph and
//! the facts and the build time that the search folder carries (see `index::mod`). A build
//! that fails half way through publishing removes the manifest, so the next query builds
//! the index again instead of trusting a mix.

use super::lock::Lock;
use super::manifest::{Counts, Entry, Generation, Manifest, Part, FORMAT, VERSION};
use super::{facts_path, graph_path, load_graph, manifest_path, open_search, part_matches, part_of, parts_present, search_path, LOCK_FILE, SEARCH_TAG};
use crate::dates::now_nanos;
use crate::fail::{Fail, Res};
use lxr_core::{hash64, FileFacts};
use lxr_extract::{extract, lang_of, walk_checked, Unreadable, WalkOpts, Walked};
use lxr_graph::Graph;
use lxr_rag::{chunk_file, Chunk, SearchIndex};
use lxr_text::Embedder;
use rayon::prelude::*;
use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::thread::sleep;
use std::time::{Duration, Instant, UNIX_EPOCH};

/// A repository with more source files than this is not indexed.
pub const MAX_FILES: usize = 20_000;

/// Fewer files than this are handled one after the other: starting a thread pool costs
/// more than it saves.
const PARALLEL_FROM: usize = 8;

/// Where the index of one repository is built from and written to.
pub struct Target<'a> {
    pub root: &'a Path,
    pub dir: &'a Path,
    pub name: &'a str,
}

/// What a refresh did.
#[derive(Debug, Default, Clone)]
pub struct Refresh {
    /// True when the graph and the search index were built again.
    pub rebuilt: bool,
    /// True when there was no usable index before (or a full rebuild was asked for).
    pub from_scratch: bool,
    /// Files that were new, changed or gone.
    pub changed: usize,
    /// Time the whole refresh took.
    pub millis: u128,
    /// True when the search index was built without an embedding model.
    pub words_only: bool,
    /// The generation of the index now on disk, as in the manifest.
    pub generation: Generation,
}

impl Refresh {
    /// The line for standard error that tells what the refresh did, if it did anything.
    pub fn note(&self) -> Option<String> {
        if !self.rebuilt {
            return None;
        }
        if self.from_scratch {
            Some(format!("pgr: index built in {} ms", self.millis))
        } else {
            Some(format!("pgr: index refreshed, {} files changed, {} ms", self.changed, self.millis))
        }
    }
}

/// One source file as the walk saw it.
struct Seen {
    path: String,
    size: u64,
    mtime: u64,
}

/// How a file compares with the manifest.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Class {
    /// Same size and time.
    Same,
    /// Same size, other time, same bytes.
    Touched,
    /// Other bytes than before.
    Changed,
    /// Not in the manifest.
    New,
}

/// The files of the repository, each compared with the manifest.
struct Plan {
    seen: Vec<Seen>,
    class: Vec<Class>,
    /// The hash the manifest holds for each file, when it holds one.
    old_hash: Vec<Option<u64>>,
    /// Files in the manifest that the walk no longer finds.
    removed: usize,
}

impl Plan {
    /// Walks the repository and compares every file with `old`.
    fn new(target: &Target, old: Option<&Manifest>) -> Res<Plan> {
        let Walked { files: paths, unreadable } = walk_checked(target.root, &WalkOpts::default());
        // A folder that cannot be read has files in it that the walk does not see. Left alone, the
        // refresh would take them for deleted files and drop them from the index.
        if let Some(problem) = unreadable.first() {
            return Err(unreadable_folder(target, problem, unreadable.len()));
        }
        if paths.len() > MAX_FILES {
            return Err(Fail::Off(format!("off for {} (more than {} source files)", target.name, MAX_FILES)));
        }
        let seen = stat_all(target.root, paths);
        let known: HashMap<&str, &Entry> = match old {
            Some(m) => m.files.iter().map(|e| (e.path.as_str(), e)).collect(),
            None => HashMap::new(),
        };
        let mut class = Vec::with_capacity(seen.len());
        let mut old_hash = Vec::with_capacity(seen.len());
        let mut matched = 0usize;
        for s in &seen {
            match known.get(s.path.as_str()) {
                None => {
                    class.push(Class::New);
                    old_hash.push(None);
                }
                Some(entry) => {
                    matched += 1;
                    old_hash.push(Some(entry.hash));
                    if entry.size == s.size && entry.mtime == s.mtime {
                        class.push(Class::Same);
                    } else if entry.size == s.size {
                        let same = fs::read(target.root.join(&s.path)).map(|b| hash64(&b) == entry.hash).unwrap_or(false);
                        class.push(if same { Class::Touched } else { Class::Changed });
                    } else {
                        class.push(Class::Changed);
                    }
                }
            }
        }
        let removed = known.len().saturating_sub(matched);
        Ok(Plan { seen, class, old_hash, removed })
    }

    /// True when the manifest describes exactly the files on disk.
    fn is_clean(&self, old: Option<&Manifest>) -> bool {
        old.is_some() && self.removed == 0 && self.class.iter().all(|c| matches!(c, Class::Same | Class::Touched))
    }

    /// Number of files that were new, changed or gone.
    fn changed(&self) -> usize {
        self.removed + self.class.iter().filter(|c| matches!(c, Class::Changed | Class::New)).count()
    }
}

/// The failure for a repository with a part that cannot be read. It names the first such folder
/// and says how many more there are; the index is left as it was.
fn unreadable_folder(target: &Target, problem: &Unreadable, count: usize) -> Fail {
    let more = match count {
        1 => String::new(),
        n => format!(" (and {} more)", n - 1),
    };
    Fail::index(format!(
        "cannot read the folder {}{more} in {}: {}; the index was not changed, because the files in it would be taken for deleted ones (fix the permissions, or add the folder to .gitignore; PGR_NO_REFRESH=1 uses the index as it is)",
        problem.path, target.name, problem.reason
    ))
}

/// The modified time of a file in nanoseconds since the Unix epoch, 0 when unknown.
fn stamp(meta: &fs::Metadata) -> u64 {
    meta.modified().ok().and_then(|t| t.duration_since(UNIX_EPOCH).ok()).map(|d| d.as_nanos() as u64).unwrap_or(0)
}

/// The size and time of every file in `paths` (paths from `root` with forward slashes).
/// Each folder is listed once and the sizes and times are taken from the listing, which
/// costs no call per file on Windows. A file the listing does not give, because it is
/// gone, is a link or has an odd name, is looked up on its own; one that is gone is left
/// out of the answer.
fn stat_all(root: &Path, paths: Vec<String>) -> Vec<Seen> {
    let mut by_folder: HashMap<&str, HashMap<&str, usize>> = HashMap::new();
    for (i, path) in paths.iter().enumerate() {
        let (folder, name) = match path.rfind('/') {
            Some(at) => (&path[..at], &path[at + 1..]),
            None => ("", path.as_str()),
        };
        by_folder.entry(folder).or_default().insert(name, i);
    }
    let mut found: Vec<Option<(u64, u64)>> = vec![None; paths.len()];
    for (folder, wanted) in &by_folder {
        let dir = if folder.is_empty() { root.to_path_buf() } else { root.join(folder) };
        let Ok(listing) = fs::read_dir(&dir) else { continue };
        for entry in listing.flatten() {
            let name = entry.file_name();
            let Some(&i) = name.to_str().and_then(|n| wanted.get(n)) else { continue };
            if let Some(meta) = entry.metadata().ok().filter(|m| m.is_file()) {
                found[i] = Some((meta.len(), stamp(&meta)));
            }
        }
    }
    let mut seen = Vec::with_capacity(paths.len());
    for (path, listed) in paths.into_iter().zip(found) {
        let facts = listed.or_else(|| {
            let meta = fs::metadata(root.join(&path)).ok().filter(|m| m.is_file())?;
            Some((meta.len(), stamp(&meta)))
        });
        if let Some((size, mtime)) = facts {
            seen.push(Seen { path, size, mtime });
        }
    }
    seen
}

/// The manifest of the index in `dir`, only when the rest of the index is there too.
fn usable_manifest(dir: &Path) -> Option<Manifest> {
    Manifest::load(dir).filter(|_| parts_present(dir))
}

/// True when the graph, the stored facts and the search folder in `dir` are the ones
/// `manifest` describes and can be loaded.
fn parts_agree(dir: &Path, manifest: &Manifest) -> bool {
    load_graph(&graph_path(dir), manifest.graph).is_some()
        && part_matches(&facts_path(dir), manifest.facts)
        && open_search(&search_path(dir), manifest.built).is_some()
}

/// What a check of the index found.
pub enum Checked {
    /// Nothing needs rebuilding.
    Clean(Refresh),
    /// Files changed, or there is no usable index. `complete` rebuilds it.
    Dirty(Dirty),
}

/// The work that remains after a check found changes.
pub struct Dirty {
    force: bool,
    /// A piece of the index could not be used: under the lock, build only if the pieces
    /// still do not agree.
    repair: bool,
    started: Instant,
}

/// A rebuild for an index whose pieces could not be loaded. The pieces may only seem broken
/// because another process was putting a new generation in place; `complete` waits for that
/// process, looks again, and builds the index again only when the pieces still do not agree.
pub fn repairing() -> Dirty {
    Dirty { force: true, repair: true, started: Instant::now() }
}

/// Looks at the repository and the manifest, and changes nothing but the manifest's
/// stored times. `force` skips the comparison: the index is to be rebuilt.
pub fn check(target: &Target, force: bool) -> Res<Checked> {
    let started = Instant::now();
    if force {
        return Ok(Checked::Dirty(Dirty { force, repair: false, started }));
    }
    let old = usable_manifest(target.dir);
    let plan = Plan::new(target, old.as_ref())?;
    if plan.is_clean(old.as_ref()) {
        return Ok(Checked::Clean(settle_clean(target, old, &plan, started)));
    }
    Ok(Checked::Dirty(Dirty { force, repair: false, started }))
}

/// Takes the lock, or says why the index cannot be made current.
fn take_lock(target: &Target) -> Res<Lock> {
    match Lock::acquire(target.dir) {
        Ok(Some(lock)) => Ok(lock),
        Ok(None) => Err(Fail::index(format!(
            "another process holds {} and did not finish in time, so the index cannot be made current (run again later; if no pgr process is running, delete that file; PGR_NO_REFRESH=1 uses the index as it is)",
            target.dir.join(LOCK_FILE).display()
        ))),
        Err(error) => Err(Fail::index(format!("cannot create the index folder {}: {error}", target.dir.display()))),
    }
}

/// Rebuilds the index under the lock, after a check found changes. The files are compared
/// again once the lock is held, because another process may have done the same work.
/// When the lock cannot be had in time, this is an error: an index that is not known to
/// match the files is not answered from.
pub fn complete(target: &Target, dirty: Dirty) -> Res<Refresh> {
    let Dirty { force, repair, started } = dirty;
    let lock = take_lock(target)?;
    if repair {
        if let Some(manifest) = usable_manifest(target.dir) {
            if parts_agree(target.dir, &manifest) {
                return Ok(Refresh { millis: started.elapsed().as_millis(), generation: manifest.generation(), ..Refresh::default() });
            }
        }
    }
    let old = if force { None } else { usable_manifest(target.dir) };
    let plan = Plan::new(target, old.as_ref())?;
    if !force && plan.is_clean(old.as_ref()) {
        let generation = match &old {
            Some(manifest) => {
                remember_times(target, manifest.built, &plan);
                manifest.generation()
            }
            None => Generation::default(),
        };
        return Ok(Refresh { millis: started.elapsed().as_millis(), generation, ..Refresh::default() });
    }
    build(target, &plan, old.as_ref(), started, &lock)
}

/// Nothing changed. When files only had their time touched, the manifest learns the new
/// times so that they are not read again next time.
fn settle_clean(target: &Target, old: Option<Manifest>, plan: &Plan, started: Instant) -> Refresh {
    let generation = match &old {
        Some(manifest) => {
            if plan.class.iter().any(|c| *c == Class::Touched) {
                // Only under the lock, and only if nobody is writing: the manifest is not
                // rewritten behind the back of a build. Without the lock the files are
                // simply hashed again next time.
                if let Ok(Some(lock)) = Lock::try_acquire(target.dir) {
                    remember_times(target, manifest.built, plan);
                    drop(lock);
                }
            }
            manifest.generation()
        }
        None => Generation::default(),
    };
    Refresh { millis: started.elapsed().as_millis(), generation, ..Refresh::default() }
}

/// Writes the times of the files in `plan` into the manifest of generation `built`. The lock
/// must be held. A manifest of another generation, that a newer build wrote, is left alone:
/// the comparison in `plan` was made against the old one.
fn remember_times(target: &Target, built: u64, plan: &Plan) {
    let Some(mut manifest) = Manifest::load(target.dir) else { return };
    if manifest.built != built {
        return;
    }
    let times: HashMap<&str, u64> = plan.seen.iter().map(|s| (s.path.as_str(), s.mtime)).collect();
    for entry in manifest.files.iter_mut() {
        if let Some(time) = times.get(entry.path.as_str()) {
            entry.mtime = *time;
        }
    }
    // A manifest that cannot be rewritten only means the files are hashed again next time.
    let _ = manifest.save(target.dir);
}

/// One file read, extracted and chunked.
struct Built {
    entry: Entry,
    facts: FileFacts,
    chunks: Vec<Chunk>,
}

/// Reads one file and gives its entry, facts and chunks. `None` when the file is gone since
/// the walk or is not source. A file that exists and cannot be read is an error: leaving it
/// out would drop its definitions from the index without a word. Stored facts are reused
/// when the bytes are the ones the manifest knows.
fn make(root: &Path, seen: &Seen, class: Class, old_hash: Option<u64>, stored: &HashMap<String, FileFacts>) -> Res<Option<Built>> {
    let bytes = match fs::read(root.join(&seen.path)) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(Fail::index(format!("cannot read {}: {error}", seen.path))),
    };
    let hash = hash64(&bytes);
    let text = String::from_utf8_lossy(&bytes);
    let reuse = match class {
        Class::Same | Class::Touched if old_hash == Some(hash) => stored.get(&seen.path),
        _ => None,
    };
    let facts = match reuse {
        Some(facts) => facts.clone(),
        None => match lang_of(Path::new(&seen.path)) {
            Some(lang) => extract(&seen.path, lang, &text),
            None => return Ok(None),
        },
    };
    let chunks = chunk_file(&facts, &text);
    Ok(Some(Built { entry: Entry { path: seen.path.clone(), size: seen.size, mtime: seen.mtime, hash }, facts, chunks }))
}

/// What building the search index left behind.
struct SearchDone {
    chunks: usize,
    vectors: bool,
    model_missing: bool,
}

/// The new pieces of an index, written beside the live ones until they are put in place.
/// The names carry this process's number, so two builds never write the same file.
struct Stage {
    graph: PathBuf,
    facts: PathBuf,
    search: PathBuf,
}

impl Stage {
    fn new(dir: &Path) -> Stage {
        Stage { graph: sibling(&graph_path(dir), "new"), facts: sibling(&facts_path(dir), "new"), search: sibling(&search_path(dir), "new") }
    }

    /// Removes what is left of the staged pieces.
    fn discard(&self) {
        let _ = fs::remove_file(&self.graph);
        let _ = fs::remove_file(&self.facts);
        let _ = fs::remove_dir_all(&self.search);
    }
}

/// Builds the graph, the stored facts, the search index and the manifest, and publishes them.
fn build(target: &Target, plan: &Plan, old: Option<&Manifest>, started: Instant, lock: &Lock) -> Res<Refresh> {
    let dir = target.dir;
    sweep(dir);
    let stored: HashMap<String, FileFacts> = match old {
        // Stored facts of another build than the manifest's are not trusted.
        Some(m) if part_matches(&facts_path(dir), m.facts) => lxr_core::load::<Vec<FileFacts>>(&facts_path(dir))
            .map(|all| all.into_iter().map(|f| (f.path.clone(), f)).collect())
            .unwrap_or_default(),
        _ => HashMap::new(),
    };
    let job = |i: usize| make(target.root, &plan.seen[i], plan.class[i], plan.old_hash[i], &stored);
    let made: Vec<Res<Option<Built>>> = if plan.seen.len() >= PARALLEL_FROM {
        (0..plan.seen.len()).into_par_iter().map(job).collect()
    } else {
        (0..plan.seen.len()).map(job).collect()
    };
    let mut entries = Vec::with_capacity(made.len());
    let mut facts = Vec::with_capacity(made.len());
    let mut chunks = Vec::new();
    for built in made {
        if let Some(built) = built? {
            entries.push(built.entry);
            facts.push(built.facts);
            chunks.extend(built.chunks);
        }
    }
    lock.touch();

    // Every build has a later number than the one before it, whatever the clock did.
    let id = match old {
        Some(m) => now_nanos().max(m.built + 1),
        None => now_nanos(),
    };
    let stage = Stage::new(dir);
    let (graph_done, search_done) = rayon::join(
        || -> io::Result<(usize, usize, Part, Part)> {
            let graph = Graph::build(&facts);
            graph.save(&stage.graph)?;
            lxr_core::save(&stage.facts, &facts)?;
            let symbols = graph.nodes.len().saturating_sub(graph.files.len());
            Ok((symbols, graph.edges.len(), part_of(&stage.graph)?, part_of(&stage.facts)?))
        },
        || build_search(&stage.search, chunks, id),
    );
    let (symbols, links, graph_part, facts_part) = match graph_done {
        Ok(done) => done,
        Err(error) => {
            stage.discard();
            return Err(Fail::index(format!("cannot write the graph: {error}")));
        }
    };
    let search = match search_done {
        Ok(done) => done,
        Err(error) => {
            stage.discard();
            return Err(Fail::index(format!("cannot write the search index: {error}")));
        }
    };

    // A build that ran so long that its lock was taken over must not publish beside the
    // build that took over.
    if !lock.is_ours() {
        stage.discard();
        return Err(Fail::index(format!("the lock {LOCK_FILE} was taken over while the index was built, so this build was dropped (run again)")));
    }
    lock.touch();
    let manifest = Manifest {
        format: FORMAT,
        version: VERSION.to_string(),
        built: id,
        embeddings: search.vectors,
        counts: Counts { files: facts.len() as u64, symbols: symbols as u64, links: links as u64, chunks: search.chunks as u64 },
        graph: graph_part,
        facts: facts_part,
        files: entries,
    };
    publish(dir, &stage, &manifest)?;
    Ok(Refresh {
        rebuilt: true,
        from_scratch: old.is_none(),
        changed: plan.changed(),
        millis: started.elapsed().as_millis(),
        words_only: search.model_missing,
        generation: manifest.generation(),
    })
}

/// Puts the staged pieces in place, the search folder first and the manifest last. A failure
/// before anything was replaced leaves the old index as it was. A failure after that leaves
/// pieces of two builds, so the manifest is removed: the next query builds the index again.
fn publish(dir: &Path, stage: &Stage, manifest: &Manifest) -> Res<()> {
    let mut replaced = false;
    let result = (|| -> io::Result<()> {
        swap_dir(&stage.search, &search_path(dir))?;
        replaced = true;
        fs::rename(&stage.facts, facts_path(dir))?;
        fs::rename(&stage.graph, graph_path(dir))?;
        manifest.save(dir)
    })();
    match result {
        Ok(()) => Ok(()),
        Err(error) => {
            let hint = if replaced { "" } else { " (does another pgr process, such as a running pgr mcp, hold the old search index open? the old index is unchanged)" };
            if replaced {
                let _ = fs::remove_file(manifest_path(dir));
            }
            stage.discard();
            Err(Fail::index(format!("cannot put the new index in place: {error}{hint}")))
        }
    }
}

/// Removes what an earlier build left in the index folder: staged pieces, a search folder
/// that was moved aside, temporary files. The lock is held, so none of it is in use. Names
/// that end with this process's number belong to this process and are left alone.
fn sweep(dir: &Path) {
    let own = format!("-{}", std::process::id());
    let Ok(listing) = fs::read_dir(dir) else { return };
    for entry in listing.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        let staged = (name.contains(".new-") || name.contains(".old-")) && !name.ends_with(&own);
        if staged || name.ends_with(".tmp") {
            let path = entry.path();
            let _ = if path.is_dir() { fs::remove_dir_all(&path) } else { fs::remove_file(&path) };
        }
    }
}

/// Builds the search index into the new folder `fresh`, and marks the folder with the number
/// of the build.
fn build_search(fresh: &Path, chunks: Vec<Chunk>, id: u64) -> io::Result<SearchDone> {
    let embedder = model();
    let _ = fs::remove_dir_all(fresh);
    let index = SearchIndex::build(chunks, embedder.as_ref(), fresh)?;
    let done = SearchDone { chunks: index.len(), vectors: index.has_vectors(), model_missing: embedder.is_none() };
    drop(index);
    drop(embedder);
    fs::write(fresh.join(SEARCH_TAG), id.to_string())?;
    Ok(done)
}

/// The embedding model, if the machine has one.
#[cfg(not(test))]
fn model() -> Option<Embedder> {
    crate::model::open()
}

/// The unit tests never use a model, so what they build does not depend on the machine.
#[cfg(test)]
fn model() -> Option<Embedder> {
    None
}

/// A folder next to `path` with a name that is this process's own: `search.new-1234`.
fn sibling(path: &Path, tag: &str) -> PathBuf {
    let mut name = path.file_name().map(|n| n.to_os_string()).unwrap_or_default();
    name.push(format!(".{tag}-{}", std::process::id()));
    path.with_file_name(name)
}

/// Puts the folder `fresh` in the place of `target`. The old folder is moved aside first. When
/// it cannot be moved (another process has files in it open) the error is given and the old
/// folder is left as it is: it is never deleted piece by piece.
fn swap_dir(fresh: &Path, target: &Path) -> io::Result<()> {
    if !target.exists() {
        return fs::rename(fresh, target);
    }
    let aside = sibling(target, "old");
    let _ = fs::remove_dir_all(&aside);
    rename_with_patience(target, &aside)?;
    if let Err(error) = fs::rename(fresh, target) {
        let _ = fs::rename(&aside, target);
        return Err(error);
    }
    let _ = fs::remove_dir_all(&aside);
    Ok(())
}

/// Renames, trying again for about a second: on Windows a folder that a scanner or another
/// process has open for a moment cannot be moved.
fn rename_with_patience(from: &Path, to: &Path) -> io::Result<()> {
    let mut tries = 0;
    loop {
        match fs::rename(from, to) {
            Ok(()) => return Ok(()),
            Err(error) if tries >= 20 => return Err(error),
            Err(_) => {
                tries += 1;
                sleep(Duration::from_millis(50));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::SystemTime;

    /// A fresh folder for one test.
    fn scratch(tag: &str) -> PathBuf {
        let base = std::env::temp_dir().join(format!("pgr-refresh-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(&base).unwrap();
        base
    }

    /// Brings the index of `root` in `dir` up to date, as a query does.
    fn bring_current(root: &Path, dir: &Path) -> Res<Refresh> {
        let target = Target { root, dir, name: "proj" };
        match check(&target, false)? {
            Checked::Clean(done) => Ok(done),
            Checked::Dirty(dirty) => complete(&target, dirty),
        }
    }

    fn set_time(path: &Path, time: SystemTime) {
        let file = fs::OpenOptions::new().write(true).open(path).unwrap();
        file.set_modified(time).unwrap();
    }

    #[test]
    fn touched_times_are_not_written_over_the_manifest_of_a_newer_build() {
        let base = scratch("settle");
        let (root, dir) = (base.join("src"), base.join("index"));
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("a.py"), "def one():\n    return 1\n").unwrap();
        bring_current(&root, &dir).unwrap();
        set_time(&root.join("a.py"), SystemTime::now() + Duration::from_secs(5));

        // This process compares the files with the manifest and finds a touched file.
        let target = Target { root: &root, dir: &dir, name: "proj" };
        let old = usable_manifest(&dir);
        let plan = Plan::new(&target, old.as_ref()).unwrap();
        assert!(plan.class.contains(&Class::Touched));

        // Before it saves, another process changes the file and builds a newer index.
        fs::write(root.join("a.py"), "def two():\n    return 2\n\n\ndef three():\n    return 3\n").unwrap();
        bring_current(&root, &dir).unwrap();
        let newer = Manifest::load(&dir).unwrap();

        let _ = settle_clean(&target, old, &plan, Instant::now());
        let after = Manifest::load(&dir).unwrap();
        assert_eq!(after.built, newer.built, "the older manifest was written over the newer one");
        assert_eq!(after.files, newer.files);
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn touched_times_are_remembered_when_the_manifest_is_still_the_one_compared() {
        let base = scratch("remember");
        let (root, dir) = (base.join("src"), base.join("index"));
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("a.py"), "def one():\n    return 1\n").unwrap();
        bring_current(&root, &dir).unwrap();
        let before = Manifest::load(&dir).unwrap();
        set_time(&root.join("a.py"), SystemTime::now() + Duration::from_secs(5));
        let done = bring_current(&root, &dir).unwrap();
        assert!(!done.rebuilt);
        let after = Manifest::load(&dir).unwrap();
        assert_eq!(after.built, before.built, "no new generation for a touched file");
        assert_ne!(after.files[0].mtime, before.files[0].mtime, "the new time is written down");
        assert!(!dir.join(LOCK_FILE).exists(), "the lock is given back");
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn a_build_that_fails_before_publishing_leaves_the_old_index_whole() {
        let base = scratch("mixture");
        let (root, dir) = (base.join("src"), base.join("index"));
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("a.py"), "def one():\n    return 1\n").unwrap();
        bring_current(&root, &dir).unwrap();
        let old = Manifest::load(&dir).unwrap();

        // The file changes, and the new search folder cannot be made: a file is in its way.
        fs::write(root.join("a.py"), "def two():\n    return 2\n\n\ndef three():\n    return 3\n").unwrap();
        fs::write(sibling(&search_path(&dir), "new"), "in the way").unwrap();
        assert!(bring_current(&root, &dir).is_err(), "the build must fail");

        let usable = usable_manifest(&dir).is_some();
        let graph = Graph::load(&graph_path(&dir)).ok();
        let graph_of_the_new_file = graph.as_ref().map_or(false, |g| !g.find("two").is_empty());
        assert!(!(usable && graph_of_the_new_file), "the manifest of the old index sits beside the graph of the new one");
        // Nothing was replaced, so the old index is there and its pieces still agree.
        assert!(usable);
        let manifest = Manifest::load(&dir).unwrap();
        assert_eq!(manifest.built, old.built);
        assert!(!graph.unwrap().find("one").is_empty());
        assert!(parts_agree(&dir, &manifest));
        assert!(!dir.join(LOCK_FILE).exists(), "the lock is given back after a failure");
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn a_failure_after_the_first_piece_was_replaced_removes_the_manifest() {
        let base = scratch("halfway");
        let (root, dir) = (base.join("src"), base.join("index"));
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("a.py"), "def one():\n    return 1\n").unwrap();
        bring_current(&root, &dir).unwrap();
        let manifest = Manifest::load(&dir).unwrap();

        // The stage and the target are both fine for the search folder, then the facts
        // cannot be put in place: a folder has the name of the file.
        let stage = Stage::new(&dir);
        fs::create_dir_all(&stage.search).unwrap();
        fs::write(stage.search.join(SEARCH_TAG), "1").unwrap();
        fs::write(&stage.graph, "g").unwrap();
        fs::write(&stage.facts, "f").unwrap();
        fs::remove_file(facts_path(&dir)).unwrap();
        fs::create_dir_all(facts_path(&dir)).unwrap();
        assert!(publish(&dir, &stage, &manifest).is_err());
        assert!(!manifest_path(&dir).exists(), "pieces of two builds must not keep a manifest");
        assert!(Manifest::load(&dir).is_none());
        assert!(!stage.graph.exists() && !stage.facts.exists() && !stage.search.exists(), "staged pieces are removed");
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn the_pieces_of_a_build_agree_with_its_manifest_and_a_foreign_piece_does_not() {
        let base = scratch("agree");
        let (root, dir) = (base.join("src"), base.join("index"));
        let (other_root, other_dir) = (base.join("other-src"), base.join("other-index"));
        fs::create_dir_all(&root).unwrap();
        fs::create_dir_all(&other_root).unwrap();
        fs::write(root.join("a.py"), "def one():\n    return 1\n").unwrap();
        fs::write(other_root.join("a.py"), "def two():\n    return 2\n\n\ndef three():\n    return 3\n").unwrap();
        bring_current(&root, &dir).unwrap();
        bring_current(&other_root, &other_dir).unwrap();
        let manifest = Manifest::load(&dir).unwrap();
        let other = Manifest::load(&other_dir).unwrap();
        assert!(parts_agree(&dir, &manifest));
        assert_eq!(manifest.generation().id, manifest.built);
        assert_ne!(other.built, manifest.built, "every build has a number of its own");

        // Each piece of another build, put in place of this one's, makes the pieces disagree.
        let tag = search_path(&dir).join(SEARCH_TAG);
        fs::copy(search_path(&other_dir).join(SEARCH_TAG), &tag).unwrap();
        assert!(!parts_agree(&dir, &manifest), "a search folder of another build must not agree");
        fs::write(&tag, manifest.built.to_string()).unwrap();
        assert!(parts_agree(&dir, &manifest));

        let graph = fs::read(graph_path(&dir)).unwrap();
        fs::copy(graph_path(&other_dir), graph_path(&dir)).unwrap();
        assert!(!parts_agree(&dir, &manifest), "a graph of another build must not agree");
        fs::write(graph_path(&dir), graph).unwrap();
        assert!(parts_agree(&dir, &manifest));

        let facts = fs::read(facts_path(&dir)).unwrap();
        fs::copy(facts_path(&other_dir), facts_path(&dir)).unwrap();
        assert!(!parts_agree(&dir, &manifest), "facts of another build must not agree");
        fs::write(facts_path(&dir), facts).unwrap();
        assert!(parts_agree(&dir, &manifest));
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn a_repair_builds_only_when_the_pieces_do_not_agree() {
        let base = scratch("repair");
        let (root, dir) = (base.join("src"), base.join("index"));
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("a.py"), "def one():\n    return 1\n").unwrap();
        bring_current(&root, &dir).unwrap();
        let built = Manifest::load(&dir).unwrap().built;
        let target = Target { root: &root, dir: &dir, name: "proj" };

        // The pieces agree (another process may have just finished): nothing is built.
        let done = complete(&target, repairing()).unwrap();
        assert!(!done.rebuilt);
        assert_eq!(done.generation.id, built);
        assert_eq!(Manifest::load(&dir).unwrap().built, built);

        // A damaged piece: the index is built again, under a number of its own.
        fs::write(graph_path(&dir), b"junk").unwrap();
        let done = complete(&target, repairing()).unwrap();
        assert!(done.rebuilt && done.from_scratch);
        let manifest = Manifest::load(&dir).unwrap();
        assert!(manifest.built > built);
        assert!(parts_agree(&dir, &manifest));
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn a_build_leaves_no_staged_files_and_sweeps_what_others_left() {
        let base = scratch("sweep");
        let (root, dir) = (base.join("src"), base.join("index"));
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("a.py"), "def one():\n    return 1\n").unwrap();
        fs::create_dir_all(&dir).unwrap();
        // Left by builds that died, or that could not remove their old folder.
        fs::create_dir_all(dir.join("search.old-999991")).unwrap();
        fs::write(dir.join("graph.bin.new-999992"), "x").unwrap();
        fs::write(dir.join("manifest.bin.999993.0.tmp"), "x").unwrap();
        // This process's own staging name is left to this process.
        fs::write(dir.join(format!("facts.bin.new-{}", std::process::id())), "x").unwrap();
        bring_current(&root, &dir).unwrap();
        let mut left: Vec<String> = fs::read_dir(&dir).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
        left.sort();
        assert!(!left.iter().any(|n| n.contains("999991") || n.contains("999992") || n.contains("999993")), "{left:?}");
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn a_file_that_cannot_be_read_is_an_error_and_a_file_that_is_gone_is_skipped() {
        let base = scratch("make");
        fs::create_dir_all(&base).unwrap();
        fs::write(base.join("a.py"), "def one():\n    return 1\n").unwrap();
        let seen = |path: &str| Seen { path: path.to_string(), size: 1, mtime: 1 };
        let stored = HashMap::new();
        assert!(make(&base, &seen("gone.py"), Class::New, None, &stored).unwrap().is_none());
        assert!(make(&base, &seen("a.py"), Class::New, None, &stored).unwrap().is_some());
        // A folder where a file is expected cannot be read as a file, and it is not "gone".
        fs::create_dir_all(base.join("b.py")).unwrap();
        let error = make(&base, &seen("b.py"), Class::New, None, &stored).err().expect("an error");
        assert!(error.text().contains("cannot read b.py"), "{}", error.text());
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn stat_all_gives_the_size_and_time_of_each_file_and_skips_the_gone() {
        let root = std::env::temp_dir().join(format!("pgr-stat-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("a/b")).unwrap();
        fs::write(root.join("top.py"), "x = 1\n").unwrap();
        fs::write(root.join("a/mid.py"), "def f():\n    pass\n").unwrap();
        fs::write(root.join("a/b/deep.py"), "").unwrap();
        let paths: Vec<String> = ["a/b/deep.py", "a/mid.py", "gone.py", "top.py"].iter().map(|s| s.to_string()).collect();
        let seen = stat_all(&root, paths);
        let names: Vec<&str> = seen.iter().map(|s| s.path.as_str()).collect();
        assert_eq!(names, ["a/b/deep.py", "a/mid.py", "top.py"]);
        for s in &seen {
            let meta = fs::metadata(root.join(&s.path)).unwrap();
            assert_eq!((s.size, s.mtime), (meta.len(), stamp(&meta)), "{}", s.path);
        }
        assert_eq!(seen[1].size, 18);
        let _ = fs::remove_dir_all(&root);
    }
}
