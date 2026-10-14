//! The state of one run: the repository, the engines that are on, and the parts of the
//! index that have been loaded so far. Every part is loaded on first use, so a command
//! pays only for what it reads.

use crate::dates::now_secs;
use crate::fail::{Fail, Res};
use crate::guard;
use crate::index::manifest::{Generation, Manifest};
use crate::index::refresh::{self, Refresh, Target};
use crate::index::{graph_path, load_graph, open_search, search_path, BANK_LOCK_FILE, MEMORY_DIR};
use crate::model;
use crate::names::{self, Kind as Name, DEFAULT_ROLE};
use crate::oslock::OsLock;
use crate::searchenv;
use lxr_core::{find_root, index_dir, index_home, repo_identity};
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

/// The graph, loaded when first asked for. `None` inside when the file cannot be read, or
/// is not the graph of the generation the slot was made for.
pub struct GraphSlot {
    path: PathBuf,
    gen: Generation,
    /// The key of the repository the graph is of (its index folder's name): the notes this view
    /// anchors carry it, and the notes it judges are those that carry it or none.
    repo: String,
    cell: OnceCell<Option<Graph>>,
}

impl GraphSlot {
    fn new(path: PathBuf, gen: Generation, repo: String) -> GraphSlot {
        GraphSlot { path, gen, repo, cell: OnceCell::new() }
    }

    /// The graph, read from disk on the first call. `None` when it cannot be read, when it
    /// is not the one the manifest describes, or when no index has been looked at yet.
    pub fn get(&self) -> Option<&Graph> {
        self.cell.get_or_init(|| if self.gen.id == 0 { None } else { load_graph(&self.path, self.gen.graph) }).as_ref()
    }
}

/// The code as the memory bank sees it, built on the graph.
impl CodeView for GraphSlot {
    fn repo(&self) -> &str {
        &self.repo
    }

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
        Some(Anchor { path: graph.path_of(found[0]).to_string(), qual: node.qual.clone(), hash: node.hash, repo: self.repo.clone() })
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
    gen: Generation,
    cell: OnceCell<Option<SearchIndex>>,
}

impl SearchSlot {
    fn new(path: PathBuf, gen: Generation) -> SearchSlot {
        SearchSlot { path, gen, cell: OnceCell::new() }
    }

    /// The search index, opened on the first call. `None` when it cannot be opened, when it
    /// is not the one of the generation the slot was made for, or when no index has been
    /// looked at yet.
    pub fn get(&self) -> Option<&SearchIndex> {
        self.cell.get_or_init(|| if self.gen.id == 0 { None } else { open_search(&self.path, self.gen.id, false) }).as_ref()
    }
}

/// The embedding model, found and opened when first asked for.
pub struct EmbedSlot {
    cell: OnceCell<Option<Embedder>>,
}

impl EmbedSlot {
    /// The model, or `None` when there is none or it cannot be opened.
    pub fn get(&self) -> Option<&Embedder> {
        self.cell.get_or_init(model::open).as_ref()
    }
}

/// Which parts of the index a command reads.
#[derive(Clone, Copy, Default)]
pub struct Needs {
    pub graph: bool,
    pub search: bool,
    /// The embedding model, for commands that use it whether or not the search index has vectors.
    pub embed: bool,
    /// The command reads the vectors of the search index (a search by words alone does not), so
    /// the thread that opens the index also puts a packed file of them back together.
    pub vectors: bool,
}

/// Parts of the index that helper threads load while the check for changed files runs.
/// The check and the loads take about as long each, so running them side by side gives
/// the answer sooner. The loaded parts are used only when the check found nothing to
/// rebuild; otherwise they are dropped and loaded again from the new index.
struct Early {
    /// The generation of the manifest the loads started from. The loads check their files
    /// against it.
    gen: Generation,
    graph: Option<JoinHandle<Option<Graph>>>,
    search: Option<JoinHandle<Option<SearchIndex>>>,
    embed: Option<JoinHandle<Option<Embedder>>>,
}

impl Early {
    /// Starts the loads that `needs` asks for and that the slots do not hold yet. `None`
    /// when there is no index to load from.
    fn start(ctx: &Ctx, needs: Needs) -> Option<Early> {
        let manifest = Manifest::load(&ctx.dir)?;
        let gen = manifest.generation();
        let graph = (needs.graph && ctx.graph.cell.get().is_none()).then(|| {
            let path = ctx.graph.path.clone();
            thread::spawn(move || load_graph(&path, gen.graph))
        });
        let wants_search = needs.search && ctx.search.cell.get().is_none();
        let warm = needs.vectors && !searchenv::leaves_model_alone();
        let search = wants_search.then(|| {
            let path = ctx.search.path.clone();
            thread::spawn(move || open_search(&path, gen.id, warm))
        });
        // A search by words alone never opens the model, so it is not loaded for one.
        let wants_model = needs.embed || (wants_search && manifest.embeddings && !searchenv::leaves_model_alone());
        let embed = (wants_model && ctx.embed.cell.get().is_none()).then(|| thread::spawn(model::open));
        Some(Early { gen, graph, search, embed })
    }

    /// Hands the loaded parts to the slots, when the index did not change meanwhile.
    fn deliver(self, ctx: &mut Ctx) {
        let same = ctx.gen == self.gen;
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

/// The names a run was started with: the flags `--bank`, `--task` and `--from`, already checked.
/// What a flag leaves out comes from `PGR_BANK`, `PGR_TASK` and `PGR_ROLE`.
#[derive(Clone, Debug, Default)]
pub struct Names {
    pub bank: Option<String>,
    pub task: Option<String>,
    pub role: Option<String>,
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
    /// The task and role the command line gave, when it did.
    flag_task: Option<String>,
    flag_role: Option<String>,
    bank: Option<Bank>,
    no_refresh: bool,
    fresh: bool,
    /// The generation of the index this run reads: what its slots check their files against.
    gen: Generation,
    repaired: bool,
}

impl Ctx {
    /// Sets up a run for the repository at `root`. The bank is the one `names` gives, else
    /// `PGR_BANK` (a bank name is checked wherever it comes from), else the bank of the repository.
    pub fn with_names(root: PathBuf, engines: Engines, names: Names) -> Res<Ctx> {
        let name = root.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| root.display().to_string());
        let dir = index_dir(&root);
        let bank_dir = match names.bank.or_else(|| names::from_env("PGR_BANK")) {
            Some(bank) => index_home().join("banks").join(names::check(Name::Bank, &bank)?),
            None => dir.join(MEMORY_DIR),
        };
        let no_refresh = std::env::var("PGR_NO_REFRESH").map(|v| !v.is_empty() && v != "0").unwrap_or(false);
        Ok(Ctx {
            graph: GraphSlot::new(graph_path(&dir), Generation::default(), repo_identity(&root)),
            search: SearchSlot::new(search_path(&dir), Generation::default()),
            embed: EmbedSlot { cell: OnceCell::new() },
            flag_task: names.task,
            flag_role: names.role,
            root,
            name,
            dir,
            bank_dir,
            engines,
            bank: None,
            no_refresh,
            fresh: false,
            gen: Generation::default(),
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

    /// Forgets that the index was checked, so that the next query checks it again, and brings
    /// the memory bank, if one is open, up to date with the notes other processes have written
    /// since. The bank is kept between requests: a look at the log's size and time is all it
    /// costs when nobody has written. A server calls this before every request.
    pub fn begin_request(&mut self) {
        self.fresh = false;
        self.catch_up_bank();
    }

    /// Lets a bank that is held read what other processes wrote since it last looked. A bank
    /// that cannot is let go, so that the next use opens it again and gives the error.
    fn catch_up_bank(&mut self) {
        if let Some(bank) = self.bank.as_mut() {
            if bank.refresh().is_err() {
                self.bank = None;
            }
        }
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
                Some(manifest) => self.adopt(manifest.generation()),
                None => return Err(Fail::index("no index yet (run: pgr refresh)")),
            }
            if let Some(early) = early {
                early.deliver(self);
            }
        } else {
            match refresh::check(&self.target(), false)? {
                refresh::Checked::Clean(done) => {
                    self.adopt(done.generation);
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
                self.adopt(done.generation);
                Ok(done)
            }
            refresh::Checked::Dirty(dirty) => self.rebuild(dirty),
        }
    }

    /// Rebuilds the index after a check found changes.
    fn rebuild(&mut self, dirty: refresh::Dirty) -> Res<Refresh> {
        // An open search index must not sit in the way of the folder swap. The slot that
        // replaces it opens the folder again when it is asked for.
        self.search = SearchSlot::new(search_path(&self.dir), self.gen);
        let done = refresh::complete(&self.target(), dirty)?;
        if let Some(line) = done.note() {
            crate::say(&line);
        }
        self.reset(done.generation);
        Ok(done)
    }

    /// Notes the generation of the index now on disk and drops what an older one gave.
    fn adopt(&mut self, gen: Generation) {
        if gen != self.gen {
            self.reset(gen);
        }
    }

    /// Makes new, empty slots that check their files against `gen`.
    fn reset(&mut self, gen: Generation) {
        self.gen = gen;
        self.graph = GraphSlot::new(graph_path(&self.dir), gen, repo_identity(&self.root));
        self.search = SearchSlot::new(search_path(&self.dir), gen);
    }

    /// Builds the index again, once, when a part of it cannot be used. If the parts only
    /// seemed wrong because another process was putting a new generation in place, nothing
    /// is built: the slots just read the new generation.
    fn repair(&mut self) -> Res<()> {
        if self.no_refresh || self.repaired {
            return Ok(());
        }
        self.repaired = true;
        self.rebuild(refresh::repairing())?;
        Ok(())
    }

    /// Makes the index current and loads the parts a command reads. The parts load while
    /// the check for changed files runs. Gives an error when a part cannot be read even
    /// after one rebuild.
    pub fn need(&mut self, needs: Needs) -> Res<()> {
        // A search that follows its hits through the graph reads the graph too.
        let needs = if needs.search && searchenv::graph_on()? && self.engines.is_on(Engine::Graph) { Needs { graph: true, ..needs } } else { needs };
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
            return Err(Fail::index("the graph cannot be read or is not the graph of this index (run: pgr refresh --force)"));
        }
        if needs.search && self.search.get().is_none() {
            return Err(Fail::index("the search index cannot be read or is not the search index of this index (run: pgr refresh --force)"));
        }
        Ok(())
    }

    /// Makes the index current and loads the graph.
    pub fn need_graph(&mut self) -> Res<()> {
        self.need(Needs { graph: true, ..Needs::default() })
    }

    /// Makes the index current and opens the search index. `vectors` says that the search reads
    /// the vectors (every search but one by words alone).
    pub fn need_search(&mut self, vectors: bool) -> Res<()> {
        self.need(Needs { search: true, vectors, ..Needs::default() })
    }

    /// Gets ready for a memory command. The notes are matched against the code, so the
    /// index is made current and the graph and the model load beside the check. With the
    /// graph engine off the notes are not matched against code and no index is needed.
    pub fn need_memory(&mut self) -> Res<()> {
        self.need_memory_with(true)
    }

    /// Like `need_memory`, and the model loads only when `embed` says the command will use it.
    pub fn need_memory_with(&mut self, embed: bool) -> Res<()> {
        self.engines.require(Engine::Memory)?;
        if !self.engines.is_on(Engine::Graph) {
            return Ok(());
        }
        self.need(Needs { graph: true, search: false, embed, vectors: false })
    }

    /// The task a call belongs to: `given` (a tool argument), else `--task`, else `PGR_TASK`.
    /// A name is checked wherever it comes from. `None` when no task was named.
    pub fn task(&self, given: Option<&str>) -> Res<Option<String>> {
        let name = given.map(str::to_string).or_else(|| self.flag_task.clone()).or_else(|| names::from_env("PGR_TASK"));
        name.map(|n| names::check(Name::Task, &n)).transpose()
    }

    /// Who is calling: `given` (a tool argument), else `--from`, else `PGR_ROLE`, else `agent`.
    pub fn role(&self, given: Option<&str>) -> Res<String> {
        let name = given.map(str::to_string).or_else(|| self.flag_role.clone()).or_else(|| names::from_env("PGR_ROLE"));
        names::check(Name::Role, name.as_deref().unwrap_or(DEFAULT_ROLE))
    }

    /// The graph, once `need_graph` succeeded.
    pub fn graph(&self) -> Res<&Graph> {
        self.graph.get().ok_or_else(|| Fail::index("the graph cannot be read or is not the graph of this index (run: pgr refresh --force)"))
    }

    /// The search index, once `need_search` succeeded.
    pub fn search_index(&self) -> Res<&SearchIndex> {
        self.search.get().ok_or_else(|| Fail::index("the search index cannot be read or is not the search index of this index (run: pgr refresh --force)"))
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

    /// The memory bank opened for reading only, or `None` when its folder does not exist. Nothing
    /// is created and nothing is written, not even the caches when the bank is dropped. The bank
    /// is read from the folder as it is now and is not the one `bank` keeps between requests.
    pub fn read_bank(&self) -> Res<Option<Bank>> {
        if !self.bank_dir.is_dir() {
            return Ok(None);
        }
        Bank::open_read_only(&self.bank_dir)
            .map(Some)
            .map_err(|e| Fail::index(format!("cannot open the memory bank {}: {e}", self.bank_dir.display())))
    }

    /// The code as the notes of a bank are judged against it: the graph, or nothing when the
    /// graph engine is off.
    pub fn code_view(&self) -> &dyn CodeView {
        if self.engines.is_on(Engine::Graph) {
            &self.graph
        } else {
            &NoCode
        }
    }

    /// Takes the lock that orders the writes of pgr processes to the memory bank, and brings the
    /// bank this run had open up to date: one opened before the lock may be behind notes written
    /// since. A bank that is not open is opened inside the lock, by using it. The lock is held
    /// until the value this gives back is dropped.
    pub fn lock_bank(&mut self) -> Res<OsLock> {
        let lock = OsLock::take(&self.bank_dir.join(BANK_LOCK_FILE))
            .map_err(|e| Fail::index(format!("cannot lock the memory bank {}: {e}", self.bank_dir.display())))?;
        self.catch_up_bank();
        Ok(lock)
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

#[cfg(test)]
mod tests {
    use super::*;
    use lxr_memory::RetainOpts;

    /// A context over a bank folder of its own.
    fn context(tag: &str) -> (Ctx, PathBuf) {
        let base = std::env::temp_dir().join(format!("pgr-ctx-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let mut ctx = Ctx::with_names(base.join("repo"), Engines::all(), Names::default()).unwrap();
        ctx.bank_dir = base.join("bank");
        (ctx, base)
    }

    /// Another process keeps a note in the bank of `ctx`.
    fn another_process_keeps(ctx: &Ctx, text: &str) {
        let mut other = Bank::open(&ctx.bank_dir).unwrap();
        other.retain(text, &RetainOpts::default(), &NoCode, None).unwrap();
        other.flush().unwrap();
    }

    #[test]
    fn a_bank_is_kept_between_requests_and_reads_what_others_wrote_meanwhile() {
        let (mut ctx, base) = context("held");
        ctx.begin_request();
        assert!(ctx.bank.is_none(), "nothing is opened for a request that does not use the bank");
        assert_eq!(ctx.bank().unwrap().len(), 0);

        another_process_keeps(&ctx, "The scheduler retries failed jobs.");
        ctx.begin_request();
        assert!(ctx.bank.is_some(), "the bank is kept for the next request, not opened again");
        assert_eq!(ctx.bank().unwrap().len(), 1, "and it has the note the other process wrote");

        // A request in which nobody wrote keeps it as it is.
        ctx.begin_request();
        assert!(ctx.bank.is_some());
        assert_eq!(ctx.bank().unwrap().len(), 1);

        another_process_keeps(&ctx, "The renderer paints widgets.");
        ctx.begin_request();
        assert_eq!(ctx.bank().unwrap().len(), 2);
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn taking_the_write_lock_keeps_the_bank_and_brings_it_up_to_date() {
        let (mut ctx, base) = context("locked");
        assert_eq!(ctx.bank().unwrap().len(), 0);
        another_process_keeps(&ctx, "The scheduler retries failed jobs.");
        let lock = ctx.lock_bank().unwrap();
        assert!(ctx.bank.is_some(), "the bank is kept");
        assert_eq!(ctx.bank().unwrap().len(), 1, "and is up to date inside the lock");
        drop(lock);
        let _ = std::fs::remove_dir_all(&base);
    }
}