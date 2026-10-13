//! The state of one run: the repository, the engines that are on, and the parts of the
//! index that have been loaded so far. Every part is loaded on first use, so a command
//! pays only for what it reads.

use crate::dates::now_secs;
use crate::fail::{Fail, Res};
use crate::guard;
use crate::index::manifest::Manifest;
use crate::index::refresh::{self, Refresh, Target};
use crate::index::{graph_path, search_path, MEMORY_DIR};
use lxr_core::{find_root, index_dir, index_home};
use lxr_graph::{Graph, NodeKind};
use lxr_memory::{Anchor, Bank, CodeView, NoCode};
use lxr_rag::SearchIndex;
use lxr_text::Embedder;
use std::cell::OnceCell;
use std::path::{Path, PathBuf};
use std::thread::{self, JoinHandle};

/// One of the three engines a command can belong to.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Engine {
    Graph,
    Search,
    Memory,
}

impl Engine {
    /// The word the flags use for the engine.
    pub fn name(self) -> &'static str {
        match self {
            Engine::Graph => "graph",
            Engine::Search => "search",
            Engine::Memory => "memory",
        }
    }
}

/// Which engines are on.
#[derive(Clone, Copy, Debug)]
pub struct Engines {
    graph: bool,
    search: bool,
    memory: bool,
}

impl Engines {
    /// All three engines on.
    pub fn all() -> Engines {
        Engines { graph: true, search: true, memory: true }
    }

    /// Reads a comma list such as `graph,memory`.
    pub fn parse(list: &str) -> Res<Engines> {
        let mut engines = Engines { graph: false, search: false, memory: false };
        let mut any = false;
        for word in list.split(',').map(|w| w.trim().to_ascii_lowercase()).filter(|w| !w.is_empty()) {
            any = true;
            match word.as_str() {
                "graph" => engines.graph = true,
                "search" => engines.search = true,
                "memory" => engines.memory = true,
                other => return Err(Fail::usage(format!("unknown engine '{other}' (use graph, search, memory)"))),
            }
        }
        if !any {
            return Err(Fail::usage("--engines needs a comma list out of graph, search, memory"));
        }
        Ok(engines)
    }

    /// True when the engine is on.
    pub fn is_on(&self, engine: Engine) -> bool {
        match engine {
            Engine::Graph => self.graph,
            Engine::Search => self.search,
            Engine::Memory => self.memory,
        }
    }

    /// Gives a usage error when the engine is off.
    pub fn require(&self, engine: Engine) -> Res<()> {
        if self.is_on(engine) {
            Ok(())
        } else {
            Err(Fail::usage(format!("the {} engine is off", engine.name())))
        }
    }
}

/// The graph, loaded when first asked for. `None` inside when the file cannot be read.
pub struct GraphSlot {
    path: PathBuf,
    cell: OnceCell<Option<Graph>>,
}

impl GraphSlot {
    fn new(path: PathBuf) -> GraphSlot {
        GraphSlot { path, cell: OnceCell::new() }
    }

    /// The graph, read from disk on the first call. `None` when it cannot be read.
    pub fn get(&self) -> Option<&Graph> {
        self.cell.get_or_init(|| Graph::load(&self.path).ok()).as_ref()
    }
}

/// The code as the memory bank sees it, built on the graph.
impl CodeView for GraphSlot {
    fn anchor(&self, word: &str) -> Option<Anchor> {
        if word.chars().count() < 2 {
            return None;
        }
        let graph = self.get()?;
        let found = graph.find(word);
        if found.len() != 1 {
            return None;
        }
        let node = graph.nodes.get(found[0] as usize)?;
        if !matches!(node.kind, NodeKind::Symbol(_)) {
            return None;
        }
        Some(Anchor { path: graph.path_of(found[0]).to_string(), qual: node.qual.clone(), hash: node.hash })
    }

    fn current_hash(&self, anchor: &Anchor) -> Option<u64> {
        let graph = self.get()?;
        let mut first = None;
        for id in graph.find(&format!("{}::{}", anchor.path, anchor.qual)) {
            let node = graph.nodes.get(id as usize)?;
            if !matches!(node.kind, NodeKind::Symbol(_)) || node.qual != anchor.qual || graph.path_of(id) != anchor.path {
                continue;
            }
            if node.hash == anchor.hash {
                return Some(node.hash);
            }
            first.get_or_insert(node.hash);
        }
        first
    }

    fn knows_code(&self) -> bool {
        self.get().is_some()
    }
}

/// The search index, opened when first asked for.
pub struct SearchSlot {
    path: PathBuf,
    cell: OnceCell<Option<SearchIndex>>,
}

impl SearchSlot {
    fn new(path: PathBuf) -> SearchSlot {
        SearchSlot { path, cell: OnceCell::new() }
    }

    /// The search index, opened on the first call. `None` when it cannot be opened.
    pub fn get(&self) -> Option<&SearchIndex> {
        self.cell.get_or_init(|| SearchIndex::open(&self.path).ok()).as_ref()
    }
}

/// The embedding model, found and opened when first asked for.
pub struct EmbedSlot {
    cell: OnceCell<Option<Embedder>>,
}

impl EmbedSlot {
    /// The model, or `None` when there is none or it cannot be opened.
    pub fn get(&self) -> Option<&Embedder> {
        self.cell.get_or_init(open_embedder).as_ref()
    }
}

/// Finds and opens the embedding model. `None` when there is none or it cannot be opened.
fn open_embedder() -> Option<Embedder> {
    Embedder::locate().and_then(|dir| Embedder::open(&dir).ok())
}

/// Which parts of the index a command reads.
#[derive(Clone, Copy, Default)]
pub struct Needs {
    pub graph: bool,
    pub search: bool,
    /// The embedding model, for commands that use it whether or not the search index has vectors.
    pub embed: bool,
}

/// Parts of the index that helper threads load while the check for changed files runs.
/// The check and the loads take about as long each, so running them side by side gives
/// the answer sooner. The loaded parts are used only when the check found nothing to
/// rebuild; otherwise they are dropped and loaded again from the new index.
struct Early {
    /// The build time of the manifest the loads started from.
    stamp: u64,
    graph: Option<JoinHandle<Option<Graph>>>,
    search: Option<JoinHandle<Option<SearchIndex>>>,
    embed: Option<JoinHandle<Option<Embedder>>>,
}

impl Early {
    /// Starts the loads that `needs` asks for and that the slots do not hold yet. `None`
    /// when there is no index to load from.
    fn start(ctx: &Ctx, needs: Needs) -> Option<Early> {
        let manifest = Manifest::load(&ctx.dir)?;
        let graph = (needs.graph && ctx.graph.cell.get().is_none()).then(|| {
            let path = ctx.graph.path.clone();
            thread::spawn(move || Graph::load(&path).ok())
        });
        let wants_search = needs.search && ctx.search.cell.get().is_none();
        let search = wants_search.then(|| {
            let path = ctx.search.path.clone();
            thread::spawn(move || SearchIndex::open(&path).ok())
        });
        // A search by words alone never opens the model, so it is not loaded for one.
        let wants_model = needs.embed || (wants_search && manifest.embeddings);
        let embed = (wants_model && ctx.embed.cell.get().is_none()).then(|| thread::spawn(open_embedder));
        Some(Early { stamp: manifest.built, graph, search, embed })
    }

    /// Hands the loaded parts to the slots, when the index did not change meanwhile.
    fn deliver(self, ctx: &mut Ctx) {
        let same = ctx.stamp == self.stamp;
        if let Some(Some(graph)) = joined(self.graph, same) {
            let _ = ctx.graph.cell.set(Some(graph));
        }
        if let Some(Some(index)) = joined(self.search, same) {
            let _ = ctx.search.cell.set(Some(index));
        }
        if let Some(model) = joined(self.embed, same) {
            let _ = ctx.embed.cell.set(model);
        }
    }

    /// Waits for the loads to finish and drops what they loaded, so that no file of the
    /// old index is held open while the index is rebuilt.
    fn discard(self) {
        let _ = joined(self.graph, true);
        let _ = joined(self.search, true);
        let _ = joined(self.embed, true);
    }
}

/// The result of a helper thread, when it is wanted and the thread did not fail.
fn joined<T>(handle: Option<JoinHandle<T>>, wanted: bool) -> Option<T> {
    if !wanted {
        return None;
    }
    handle?.join().ok()
}

/// Everything one run knows.
pub struct Ctx {
    /// The repository root.
    pub root: PathBuf,
    /// The name shown in status lines: the folder name of the root.
    pub name: String,
    /// The index folder of the repository.
    pub dir: PathBuf,
    /// The memory bank folder.
    pub bank_dir: PathBuf,
    pub engines: Engines,
    pub graph: GraphSlot,
    pub search: SearchSlot,
    pub embed: EmbedSlot,
    bank: Option<Bank>,
    no_refresh: bool,
    fresh: bool,
    stamp: u64,
    repaired: bool,
}

impl Ctx {
    /// Sets up a run for the repository at `root`.
    pub fn new(root: PathBuf, engines: Engines) -> Res<Ctx> {
        let name = root.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| root.display().to_string());
        let dir = index_dir(&root);
        let bank_dir = match std::env::var("PGR_BANK").ok().filter(|v| !v.is_empty()) {
            Some(bank) => {
                if bank.contains(['/', '\\', ':']) || bank == "." || bank == ".." {
                    return Err(Fail::usage(format!("PGR_BANK '{bank}' must be a plain name")));
                }
                index_home().join("banks").join(bank)
            }
            None => dir.join(MEMORY_DIR),
        };
        let no_refresh = std::env::var("PGR_NO_REFRESH").map(|v| !v.is_empty() && v != "0").unwrap_or(false);
        Ok(Ctx {
            graph: GraphSlot::new(graph_path(&dir)),
            search: SearchSlot::new(search_path(&dir)),
            embed: EmbedSlot { cell: OnceCell::new() },
            root,
            name,
            dir,
            bank_dir,
            engines,
            bank: None,
            no_refresh,
            fresh: false,
            stamp: 0,
            repaired: false,
        })
    }

    /// The repository root for a `--root` flag, or for the folder the program runs in.
    pub fn resolve_root(flag: Option<&Path>) -> Res<PathBuf> {
        match flag {
            Some(path) => {
                let full = std::fs::canonicalize(path)
                    .map_err(|_| Fail::usage(format!("--root {} is not a folder", path.display())))?;
                if !full.is_dir() {
                    return Err(Fail::usage(format!("--root {} is not a folder", path.display())));
                }
                Ok(plain(&full))
            }
            None => {
                let here = std::env::current_dir().map_err(|e| Fail::index(format!("cannot tell the current folder: {e}")))?;
                Ok(find_root(&here))
            }
        }
    }

    /// Forgets that the index was checked, so that the next query checks it again.
    /// A server calls this before every request.
    pub fn begin_request(&mut self) {
        self.fresh = false;
    }

    /// Gives `Fail::Off` when pgr must not index this folder.
    pub fn check_folder(&self) -> Res<()> {
        guard::check(&self.root, &self.name)
    }

    fn target(&self) -> Target<'_> {
        Target { root: &self.root, dir: &self.dir, name: &self.name }
    }

    /// Makes the index current, once per request. Builds it when it is missing and
    /// refreshes it when any file was changed, added or removed. With `PGR_NO_REFRESH`
    /// the index is used as it is. Parts of the index may already be loading on helper
    /// threads; they are kept when the check finds the index current and dropped when it
    /// has to be rebuilt.
    fn ensure_with(&mut self, early: Option<Early>) -> Res<()> {
        if self.fresh {
            return Ok(());
        }
        self.check_folder()?;
        if self.no_refresh {
            match Manifest::load(&self.dir) {
                Some(manifest) => self.adopt(manifest.built),
                None => return Err(Fail::index("no index yet (run: pgr refresh)")),
            }
            if let Some(early) = early {
                early.deliver(self);
            }
        } else {
            match refresh::check(&self.target(), false)? {
                refresh::Checked::Clean(done) => {
                    self.adopt(done.stamp);
                    if let Some(early) = early {
                        early.deliver(self);
                    }
                }
                refresh::Checked::Dirty(dirty) => {
                    // What the helper threads hold belongs to the index that is about to be replaced.
                    if let Some(early) = early {
                        early.discard();
                    }
                    self.rebuild(dirty)?;
                }
            }
        }
        self.fresh = true;
        Ok(())
    }

    /// Runs a refresh, reports it on standard error and drops what was loaded from the
    /// old index. Used by `pgr refresh` itself.
    pub fn run_refresh(&mut self, force: bool) -> Res<Refresh> {
        match refresh::check(&self.target(), force)? {
            refresh::Checked::Clean(done) => {
                self.adopt(done.stamp);
                Ok(done)
            }
            refresh::Checked::Dirty(dirty) => self.rebuild(dirty),
        }
    }

    /// Rebuilds the index after a check found changes.
    fn rebuild(&mut self, dirty: refresh::Dirty) -> Res<Refresh> {
        // An open search index must not sit in the way of the folder swap.
        self.search = SearchSlot::new(search_path(&self.dir));
        let done = refresh::complete(&self.target(), dirty)?;
        if let Some(line) = done.note() {
            crate::say(&line);
        }
        self.adopt(done.stamp);
        Ok(done)
    }

    /// Notes the build time of the index now on disk and drops what an older one gave.
    fn adopt(&mut self, stamp: u64) {
        if stamp != self.stamp {
            self.stamp = stamp;
            self.graph = GraphSlot::new(graph_path(&self.dir));
            self.search = SearchSlot::new(search_path(&self.dir));
        }
    }

    /// Rebuilds the whole index once when a part of it cannot be read.
    fn repair(&mut self) -> Res<()> {
        if self.no_refresh || self.repaired {
            return Ok(());
        }
        self.repaired = true;
        self.graph = GraphSlot::new(graph_path(&self.dir));
        self.search = SearchSlot::new(search_path(&self.dir));
        self.run_refresh(true)?;
        Ok(())
    }

    /// Makes the index current and loads the parts a command reads. The parts load while
    /// the check for changed files runs. Gives an error when a part cannot be read even
    /// after one rebuild.
    pub fn need(&mut self, needs: Needs) -> Res<()> {
        if needs.graph {
            self.engines.require(Engine::Graph)?;
        }
        if needs.search {
            self.engines.require(Engine::Search)?;
        }
        let early = if self.fresh {
            None
        } else {
            // A folder pgr must not index gets no loads either.
            self.check_folder()?;
            Early::start(self, needs)
        };
        self.ensure_with(early)?;
        if needs.graph && self.graph.get().is_none() {
            self.repair()?;
        }
        if needs.search && self.search.get().is_none() {
            self.repair()?;
        }
        if needs.graph && self.graph.get().is_none() {
            return Err(Fail::index("the graph cannot be read (run: pgr refresh --force)"));
        }
        if needs.search && self.search.get().is_none() {
            return Err(Fail::index("the search index cannot be read (run: pgr refresh --force)"));
        }
        Ok(())
    }

    /// Makes the index current and loads the graph.
    pub fn need_graph(&mut self) -> Res<()> {
        self.need(Needs { graph: true, ..Needs::default() })
    }

    /// Makes the index current and opens the search index.
    pub fn need_search(&mut self) -> Res<()> {
        self.need(Needs { search: true, ..Needs::default() })
    }

    /// Gets ready for a memory command. The notes are matched against the code, so the
    /// index is made current and the graph and the model load beside the check. With the
    /// graph engine off the notes are not matched against code and no index is needed.
    pub fn need_memory(&mut self) -> Res<()> {
        self.engines.require(Engine::Memory)?;
        if !self.engines.is_on(Engine::Graph) {
            return Ok(());
        }
        self.need(Needs { graph: true, search: false, embed: true })
    }

    /// The graph, once `need_graph` succeeded.
    pub fn graph(&self) -> Res<&Graph> {
        self.graph.get().ok_or_else(|| Fail::index("the graph cannot be read (run: pgr refresh --force)"))
    }

    /// The search index, once `need_search` succeeded.
    pub fn search_index(&self) -> Res<&SearchIndex> {
        self.search.get().ok_or_else(|| Fail::index("the search index cannot be read (run: pgr refresh --force)"))
    }

    /// The memory bank, opened (and created) on first use.
    pub fn bank(&mut self) -> Res<&mut Bank> {
        if self.bank.is_none() {
            let opened = Bank::open(&self.bank_dir)
                .map_err(|e| Fail::index(format!("cannot open the memory bank {}: {e}", self.bank_dir.display())))?;
            self.bank = Some(opened);
        }
        match self.bank.as_mut() {
            Some(bank) => Ok(bank),
            None => Err(Fail::index("the memory bank is not open")),
        }
    }

    /// Runs `work` with the open bank, a view of the code (the graph, or none when the graph
    /// engine is off) and the embedding model, if there is one.
    pub fn bank_session<R>(&mut self, work: impl FnOnce(&mut Bank, &dyn CodeView, Option<&Embedder>) -> R) -> Res<R> {
        self.bank()?;
        let embedder = self.embed.get();
        let code: &dyn CodeView = if self.engines.is_on(Engine::Graph) { &self.graph } else { &NoCode };
        match self.bank.as_mut() {
            Some(bank) => Ok(work(bank, code, embedder)),
            None => Err(Fail::index("the memory bank is not open")),
        }
    }

    /// True when the bank folder exists and holds at least one note. Opens nothing when
    /// the folder is missing, so that asking does not create it.
    pub fn bank_has_notes(&mut self) -> bool {
        if self.bank.is_none() && !self.bank_dir.is_dir() {
            return false;
        }
        self.bank().map(|bank| !bank.is_empty()).unwrap_or(false)
    }

    /// The current time in Unix seconds.
    pub fn now(&self) -> i64 {
        now_secs()
    }
}

/// On Windows `canonicalize` gives a `\\?\` path. This gives the ordinary form back.
fn plain(path: &Path) -> PathBuf {
    let text = path.to_string_lossy();
    match text.strip_prefix(r"\\?\") {
        Some(rest) if !rest.starts_with("UNC") => PathBuf::from(rest),
        _ => path.to_path_buf(),
    }
}
