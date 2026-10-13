//! Making the index current.
//!
//! One walk with sizes and times tells what changed. A file is read and hashed only
//! when its size is the same and its time differs. When nothing changed, nothing else
//! is done. Otherwise the changed and new files are extracted (in parallel when there
//! are many), the stored facts of the others are reused, and the graph and the search
//! index are built again and renamed into place under a lock, the manifest last.

use super::lock::Lock;
use super::manifest::{Counts, Entry, Manifest, FORMAT, VERSION};
use super::{facts_path, graph_path, parts_present, search_path};
use crate::dates::now_nanos;
use crate::fail::{Fail, Res};
use lxr_core::{hash64, FileFacts};
use lxr_extract::{extract, lang_of, walk, WalkOpts};
use lxr_graph::Graph;
use lxr_rag::{chunk_file, Chunk, SearchIndex};
use lxr_text::Embedder;
use rayon::prelude::*;
use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{Instant, UNIX_EPOCH};

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
    /// True when another process held the lock for too long and the index was left as it was.
    pub waited_out: bool,
    /// The build time of the index now on disk, as in the manifest.
    pub stamp: u64,
}

impl Refresh {
    /// The line for standard error that tells what the refresh did, if it did anything.
    pub fn note(&self) -> Option<String> {
        if self.waited_out {
            return Some("pgr: another refresh is still running, answering from the index as it is".to_string());
        }
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
        let paths = walk(target.root, &WalkOpts::default());
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
    started: Instant,
}

/// Looks at the repository and the manifest, and changes nothing but the manifest's
/// stored times. `force` skips the comparison: the index is to be rebuilt.
pub fn check(target: &Target, force: bool) -> Res<Checked> {
    let started = Instant::now();
    if force {
        return Ok(Checked::Dirty(Dirty { force, started }));
    }
    let old = usable_manifest(target.dir);
    let plan = Plan::new(target, old.as_ref())?;
    if plan.is_clean(old.as_ref()) {
        return Ok(Checked::Clean(settle_clean(target, old, &plan, started)));
    }
    Ok(Checked::Dirty(Dirty { force, started }))
}

/// Rebuilds the index under the lock, after a check found changes. The files are compared
/// again once the lock is held, because another process may have done the same work.
pub fn complete(target: &Target, dirty: Dirty) -> Res<Refresh> {
    let Dirty { force, started } = dirty;
    let lock = match Lock::acquire(target.dir) {
        Ok(Some(lock)) => lock,
        Ok(None) => {
            return match Manifest::load(target.dir) {
                Some(m) => Ok(Refresh { waited_out: true, stamp: m.built, millis: started.elapsed().as_millis(), ..Refresh::default() }),
                None => Err(Fail::index("another refresh holds the lock and there is no index yet")),
            };
        }
        Err(error) => return Err(Fail::index(format!("cannot create the index folder {}: {error}", target.dir.display()))),
    };
    let old = if force { None } else { usable_manifest(target.dir) };
    let plan = Plan::new(target, old.as_ref())?;
    if !force && plan.is_clean(old.as_ref()) {
        let result = settle_clean(target, old, &plan, started);
        drop(lock);
        return Ok(result);
    }
    let result = build(target, &plan, old.as_ref(), started);
    drop(lock);
    result
}

/// Nothing changed. When files only had their time touched, the manifest learns the new
/// times so that they are not read again next time.
fn settle_clean(target: &Target, old: Option<Manifest>, plan: &Plan, started: Instant) -> Refresh {
    let mut stamp_now = 0;
    if let Some(mut manifest) = old {
        stamp_now = manifest.built;
        if plan.class.iter().any(|c| *c == Class::Touched) {
            let times: HashMap<&str, u64> = plan.seen.iter().map(|s| (s.path.as_str(), s.mtime)).collect();
            for entry in manifest.files.iter_mut() {
                if let Some(time) = times.get(entry.path.as_str()) {
                    entry.mtime = *time;
                }
            }
            // A manifest that cannot be rewritten only means the files are hashed again next time.
            let _ = manifest.save(target.dir);
        }
    }
    Refresh { millis: started.elapsed().as_millis(), stamp: stamp_now, ..Refresh::default() }
}

/// One file read, extracted and chunked.
struct Built {
    entry: Entry,
    facts: FileFacts,
    chunks: Vec<Chunk>,
}

/// Reads one file and gives its entry, facts and chunks. `None` when the file is gone or
/// is not source. Stored facts are reused when the bytes are the ones the manifest knows.
fn make(root: &Path, seen: &Seen, class: Class, old_hash: Option<u64>, stored: &HashMap<String, FileFacts>) -> Option<Built> {
    let bytes = fs::read(root.join(&seen.path)).ok()?;
    let hash = hash64(&bytes);
    let text = String::from_utf8_lossy(&bytes);
    let reuse = match class {
        Class::Same | Class::Touched if old_hash == Some(hash) => stored.get(&seen.path),
        _ => None,
    };
    let facts = match reuse {
        Some(facts) => facts.clone(),
        None => extract(&seen.path, lang_of(Path::new(&seen.path))?, &text),
    };
    let chunks = chunk_file(&facts, &text);
    Some(Built { entry: Entry { path: seen.path.clone(), size: seen.size, mtime: seen.mtime, hash }, facts, chunks })
}

/// What building the search index left behind.
struct SearchDone {
    chunks: usize,
    vectors: bool,
    model_missing: bool,
}

/// Builds the graph, the stored facts, the search index and the manifest.
fn build(target: &Target, plan: &Plan, old: Option<&Manifest>, started: Instant) -> Res<Refresh> {
    let dir = target.dir;
    let stored: HashMap<String, FileFacts> = match old {
        Some(_) => lxr_core::load::<Vec<FileFacts>>(&facts_path(dir))
            .map(|all| all.into_iter().map(|f| (f.path.clone(), f)).collect())
            .unwrap_or_default(),
        None => HashMap::new(),
    };
    let job = |i: usize| make(target.root, &plan.seen[i], plan.class[i], plan.old_hash[i], &stored);
    let made: Vec<Option<Built>> = if plan.seen.len() >= PARALLEL_FROM {
        (0..plan.seen.len()).into_par_iter().map(job).collect()
    } else {
        (0..plan.seen.len()).map(job).collect()
    };
    let mut entries = Vec::with_capacity(made.len());
    let mut facts = Vec::with_capacity(made.len());
    let mut chunks = Vec::new();
    for built in made.into_iter().flatten() {
        entries.push(built.entry);
        facts.push(built.facts);
        chunks.extend(built.chunks);
    }

    let (graph_done, search_done) = rayon::join(
        || -> io::Result<(usize, usize)> {
            let graph = Graph::build(&facts);
            graph.save(&graph_path(dir))?;
            lxr_core::save(&facts_path(dir), &facts)?;
            Ok((graph.nodes.len().saturating_sub(graph.files.len()), graph.edges.len()))
        },
        || -> io::Result<SearchDone> { build_search(dir, chunks) },
    );
    let (symbols, links) = graph_done.map_err(|e| Fail::index(format!("cannot write the graph: {e}")))?;
    let search = search_done.map_err(|e| Fail::index(format!("cannot write the search index: {e}")))?;

    let manifest = Manifest {
        format: FORMAT,
        version: VERSION.to_string(),
        built: now_nanos(),
        embeddings: search.vectors,
        counts: Counts { files: facts.len() as u64, symbols: symbols as u64, links: links as u64, chunks: search.chunks as u64 },
        files: entries,
    };
    manifest.save(dir).map_err(|e| Fail::index(format!("cannot write the manifest: {e}")))?;
    Ok(Refresh {
        rebuilt: true,
        from_scratch: old.is_none(),
        changed: plan.changed(),
        millis: started.elapsed().as_millis(),
        words_only: search.model_missing,
        waited_out: false,
        stamp: manifest.built,
    })
}

/// Builds the search index into a new folder and renames it over the old one.
fn build_search(dir: &Path, chunks: Vec<Chunk>) -> io::Result<SearchDone> {
    let embedder = Embedder::locate().and_then(|model| Embedder::open(&model).ok());
    let fresh = sibling(&search_path(dir), "new");
    let _ = fs::remove_dir_all(&fresh);
    let index = SearchIndex::build(chunks, embedder.as_ref(), &fresh)?;
    let done = SearchDone { chunks: index.len(), vectors: index.has_vectors(), model_missing: embedder.is_none() };
    drop(index);
    drop(embedder);
    swap_dir(&fresh, &search_path(dir))?;
    Ok(done)
}

/// A folder next to `path` with a name that is this process's own: `search.new-1234`.
fn sibling(path: &Path, tag: &str) -> PathBuf {
    let mut name = path.file_name().map(|n| n.to_os_string()).unwrap_or_default();
    name.push(format!(".{tag}-{}", std::process::id()));
    path.with_file_name(name)
}

/// Puts the folder `fresh` in the place of `target`. The old folder is moved aside first,
/// and removed outright when it cannot be moved.
fn swap_dir(fresh: &Path, target: &Path) -> io::Result<()> {
    if !target.exists() {
        return fs::rename(fresh, target);
    }
    let aside = sibling(target, "old");
    let _ = fs::remove_dir_all(&aside);
    if fs::rename(target, &aside).is_err() {
        fs::remove_dir_all(target)?;
    }
    fs::rename(fresh, target)?;
    let _ = fs::remove_dir_all(&aside);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

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
