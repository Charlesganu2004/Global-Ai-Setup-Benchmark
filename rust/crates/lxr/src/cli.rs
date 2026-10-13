//! The command line: global flags, the command, and each command's own flags.

use crate::ctx::{Engine, Engines};
use crate::dates::parse_date;
use crate::fail::{Fail, Res};
use lexopt::{Arg, Parser};
use lxr_memory::{Budget, Kind};
use lxr_rag::Mode;
use std::ffi::OsString;
use std::path::PathBuf;

/// Default token budgets and limits. The flags can change every one of them.
pub const DEFAULT_FIND_K: usize = 5;
pub const DEFAULT_HUBS_K: usize = 10;
pub const DEFAULT_QUERY_BUDGET: usize = 600;
pub const DEFAULT_PACK_BUDGET: usize = 800;
pub const DEFAULT_DEPTH: u32 = 2;
pub const DEFAULT_RECALL_K: usize = 8;
pub const DEFAULT_RECALL_TOKENS: usize = 400;
pub const DEFAULT_LIST_K: usize = 20;

/// The short help, one line per command.
pub const HELP: &str = "\
pgr: a code graph, hybrid code search and memory for the repository you work in

usage: pgr [--root DIR] [--engines LIST] COMMAND ...

index:
  refresh [--force]        build or update the index, then print the status line
  status                   print the status line (never builds)
graph:
  def NAME...              where a name is defined
  callers NAME...          who calls it
  about NAME...            definition, doc, callers and what it calls
  affected NAME [--depth N]  what a change can reach (default depth 2)
  path A B                 shortest chain of links between two definitions
  hubs [-k N]              the most connected definitions
  explain NAME             everything the graph knows about one definition
  query QUESTION [--budget N]  a neighbourhood of the graph for a question
  map [--budget N]         the largest groups of the code
search:
  find WORDS... [-k N] [--words | --meaning]  hybrid code search
memory:
  retain TEXT... [--kind world|experience|directive] [--tag T]... [--context C] [--when DATE] [--whole]
  recall WORDS... [-k N] [--budget low|mid|high] [--max-tokens N] [--tag T]... [--since DATE] [--until DATE]
  reflect WORDS...         notes sorted into directives, observations and facts
  memory list [-k N] | forget ID | stats | path
combined:
  ask KIND:WHAT...         several questions at once (def callers about affected explain find recall path)
  pack TASK [--budget N]   one answer for a task: search hits, graph walk and notes
  mcp                      Model Context Protocol server on standard input and output
other:
  --version | -V           print the version
  help | --help | -h       this text

engines: --engines graph,search,memory (or PGR_ENGINES); PGR_NO_REFRESH=1 answers from the index as it is.
A word that starts with a dash needs -- before it.
";

/// The flags that come before or after any command.
#[derive(Default, Debug)]
pub struct Global {
    pub root: Option<PathBuf>,
    pub engines: Option<String>,
}

impl Global {
    /// The engines that are on: the flag, else `PGR_ENGINES`, else all three.
    pub fn engines(&self) -> Res<Engines> {
        if let Some(list) = &self.engines {
            return Engines::parse(list);
        }
        match std::env::var("PGR_ENGINES") {
            Ok(list) if !list.trim().is_empty() => Engines::parse(&list),
            _ => Ok(Engines::all()),
        }
    }
}

/// Arguments of `find`.
#[derive(Clone, Debug)]
pub struct FindArgs {
    pub query: String,
    pub k: usize,
    pub mode: Mode,
}

/// Arguments of `retain`.
#[derive(Clone, Debug)]
pub struct RetainArgs {
    pub text: String,
    pub kind: Kind,
    pub tags: Vec<String>,
    pub context: String,
    pub when: Option<i64>,
    pub whole: bool,
}

/// Arguments of `recall`.
#[derive(Clone, Debug)]
pub struct RecallArgs {
    pub query: String,
    pub k: usize,
    pub budget: Budget,
    pub max_tokens: usize,
    pub tags: Vec<String>,
    pub since: Option<i64>,
    pub until: Option<i64>,
}

impl RecallArgs {
    /// A recall with the default limits.
    pub fn new(query: String) -> RecallArgs {
        RecallArgs {
            query,
            k: DEFAULT_RECALL_K,
            budget: Budget::Mid,
            max_tokens: DEFAULT_RECALL_TOKENS,
            tags: Vec::new(),
            since: None,
            until: None,
        }
    }
}

/// The `memory` sub-commands.
#[derive(Clone, Debug)]
pub enum MemoryCmd {
    List { k: usize },
    Forget { id: String },
    Stats,
    Path,
}

/// Every command.
#[derive(Clone, Debug)]
pub enum Command {
    Refresh { force: bool },
    Status,
    Def { names: Vec<String> },
    Callers { names: Vec<String> },
    About { names: Vec<String> },
    Affected { name: String, depth: u32 },
    Path { from: String, to: String },
    Hubs { k: usize },
    Explain { name: String },
    Query { question: String, budget: usize },
    Map { budget: usize },
    Find(FindArgs),
    Retain(RetainArgs),
    Recall(RecallArgs),
    Reflect { query: String },
    Memory(MemoryCmd),
    Ask { questions: Vec<String> },
    Pack { task: String, budget: usize },
    Mcp,
    Help,
    Version,
}

impl Command {
    /// The engine the command belongs to, when it belongs to one.
    pub fn engine(&self) -> Option<Engine> {
        match self {
            Command::Def { .. }
            | Command::Callers { .. }
            | Command::About { .. }
            | Command::Affected { .. }
            | Command::Path { .. }
            | Command::Hubs { .. }
            | Command::Explain { .. }
            | Command::Query { .. }
            | Command::Map { .. } => Some(Engine::Graph),
            Command::Find(_) => Some(Engine::Search),
            Command::Retain(_) | Command::Recall(_) | Command::Reflect { .. } | Command::Memory(_) => Some(Engine::Memory),
            _ => None,
        }
    }
}

/// A parsed command line.
pub struct Cli {
    pub global: Global,
    pub command: Command,
}

/// One piece of the command line, with its text owned.
enum Item {
    Flag(String),
    Word(String),
    Help,
    Version,
}

fn bad(error: lexopt::Error) -> Fail {
    Fail::usage(error.to_string())
}

fn lossy(value: OsString) -> String {
    value.into_string().unwrap_or_else(|v| v.to_string_lossy().into_owned())
}

/// The value after the flag that was just read.
fn text(parser: &mut Parser, flag: &str) -> Res<String> {
    match parser.value() {
        Ok(v) => Ok(lossy(v)),
        Err(_) => Err(Fail::usage(format!("{flag} needs a value"))),
    }
}

/// A whole number after the flag, at least `least`.
fn number(parser: &mut Parser, flag: &str, least: usize) -> Res<usize> {
    let value = text(parser, flag)?;
    match value.parse::<usize>() {
        Ok(n) if n >= least => Ok(n),
        _ => Err(Fail::usage(format!("{flag} needs a whole number of at least {least}, got '{value}'"))),
    }
}

/// A date after the flag.
fn date(parser: &mut Parser, flag: &str, end_of_day: bool) -> Res<i64> {
    let value = text(parser, flag)?;
    parse_date(&value, end_of_day)
        .ok_or_else(|| Fail::usage(format!("{flag} needs a date such as 2026-10-12 or 2026-10-12T14:30, got '{value}'")))
}

/// The next flag or word. The global flags are taken here, wherever they stand.
fn next_item(parser: &mut Parser, global: &mut Global) -> Res<Option<Item>> {
    loop {
        match parser.next().map_err(bad)? {
            None => return Ok(None),
            Some(Arg::Long("root")) => {
                let value = text(parser, "--root")?;
                global.root = Some(PathBuf::from(value));
            }
            Some(Arg::Long("engines")) => {
                global.engines = Some(text(parser, "--engines")?);
            }
            Some(Arg::Long("help")) | Some(Arg::Short('h')) => return Ok(Some(Item::Help)),
            Some(Arg::Long("version")) | Some(Arg::Short('V')) => return Ok(Some(Item::Version)),
            Some(Arg::Short(c)) => return Ok(Some(Item::Flag(format!("-{c}")))),
            Some(Arg::Long(name)) => return Ok(Some(Item::Flag(format!("--{name}")))),
            Some(Arg::Value(v)) => return Ok(Some(Item::Word(lossy(v)))),
        }
    }
}

fn unknown(flag: &str) -> Fail {
    Fail::usage(format!("unknown option {flag} (try: pgr help)"))
}

/// Parses the whole command line (without the program name).
pub fn parse<I>(args: I) -> Res<Cli>
where
    I: IntoIterator,
    I::Item: Into<OsString>,
{
    let mut parser = Parser::from_args(args);
    let mut global = Global::default();
    let name = loop {
        match next_item(&mut parser, &mut global)? {
            None => return Err(Fail::NoCommand),
            Some(Item::Help) => return Ok(Cli { global, command: Command::Help }),
            Some(Item::Version) => return Ok(Cli { global, command: Command::Version }),
            Some(Item::Word(word)) => break word,
            Some(Item::Flag(flag)) => return Err(unknown(&flag)),
        }
    };
    let command = parse_command(&name, &mut parser, &mut global)?;
    Ok(Cli { global, command })
}

/// Collects the words of a command that takes only words.
fn words_only(parser: &mut Parser, global: &mut Global) -> Res<Result<Vec<String>, Command>> {
    let mut words = Vec::new();
    while let Some(item) = next_item(parser, global)? {
        match item {
            Item::Word(w) => words.push(w),
            Item::Help => return Ok(Err(Command::Help)),
            Item::Version => return Ok(Err(Command::Version)),
            Item::Flag(f) => return Err(unknown(&f)),
        }
    }
    Ok(Ok(words))
}

fn need_words(command: &str, words: Vec<String>) -> Res<Vec<String>> {
    if words.is_empty() {
        Err(Fail::usage(format!("{command} needs at least one name")))
    } else {
        Ok(words)
    }
}

fn parse_command(name: &str, parser: &mut Parser, global: &mut Global) -> Res<Command> {
    macro_rules! only_words {
        () => {
            match words_only(parser, global)? {
                Ok(words) => words,
                Err(other) => return Ok(other),
            }
        };
    }
    match name {
        "help" => Ok(Command::Help),
        "refresh" => {
            let mut force = false;
            while let Some(item) = next_item(parser, global)? {
                match item {
                    Item::Flag(f) if f == "--force" => force = true,
                    Item::Help => return Ok(Command::Help),
                    Item::Version => return Ok(Command::Version),
                    Item::Flag(f) => return Err(unknown(&f)),
                    Item::Word(w) => return Err(Fail::usage(format!("refresh takes no argument, got '{w}'"))),
                }
            }
            Ok(Command::Refresh { force })
        }
        "status" | "mcp" => {
            let extra = only_words!();
            if let Some(first) = extra.first() {
                return Err(Fail::usage(format!("{name} takes no argument, got '{first}'")));
            }
            Ok(if name == "status" { Command::Status } else { Command::Mcp })
        }
        "def" => Ok(Command::Def { names: need_words("def", only_words!())? }),
        "callers" => Ok(Command::Callers { names: need_words("callers", only_words!())? }),
        "about" => Ok(Command::About { names: need_words("about", only_words!())? }),
        "explain" => {
            let words = need_words("explain", only_words!())?;
            if words.len() > 1 {
                return Err(Fail::usage("explain takes one name"));
            }
            Ok(Command::Explain { name: words.into_iter().next().unwrap_or_default() })
        }
        "path" => {
            let words = only_words!();
            if words.len() != 2 {
                return Err(Fail::usage("path needs two names: pgr path A B"));
            }
            let mut it = words.into_iter();
            Ok(Command::Path { from: it.next().unwrap_or_default(), to: it.next().unwrap_or_default() })
        }
        "affected" => {
            let mut words = Vec::new();
            let mut depth = DEFAULT_DEPTH;
            while let Some(item) = next_item(parser, global)? {
                match item {
                    Item::Word(w) => words.push(w),
                    Item::Flag(f) if f == "--depth" => depth = number(parser, "--depth", 1)? as u32,
                    Item::Help => return Ok(Command::Help),
                    Item::Version => return Ok(Command::Version),
                    Item::Flag(f) => return Err(unknown(&f)),
                }
            }
            if words.len() != 1 {
                return Err(Fail::usage("affected needs one name: pgr affected NAME [--depth N]"));
            }
            Ok(Command::Affected { name: words.remove(0), depth })
        }
        "hubs" => {
            let mut k = DEFAULT_HUBS_K;
            while let Some(item) = next_item(parser, global)? {
                match item {
                    Item::Flag(f) if f == "-k" => k = number(parser, "-k", 1)?,
                    Item::Help => return Ok(Command::Help),
                    Item::Version => return Ok(Command::Version),
                    Item::Flag(f) => return Err(unknown(&f)),
                    Item::Word(w) => return Err(Fail::usage(format!("hubs takes no argument, got '{w}'"))),
                }
            }
            Ok(Command::Hubs { k })
        }
        "query" | "pack" | "map" => {
            let mut words = Vec::new();
            let mut budget = if name == "pack" { DEFAULT_PACK_BUDGET } else { DEFAULT_QUERY_BUDGET };
            while let Some(item) = next_item(parser, global)? {
                match item {
                    Item::Word(w) => words.push(w),
                    Item::Flag(f) if f == "--budget" => budget = number(parser, "--budget", 1)?,
                    Item::Help => return Ok(Command::Help),
                    Item::Version => return Ok(Command::Version),
                    Item::Flag(f) => return Err(unknown(&f)),
                }
            }
            let joined = words.join(" ");
            match name {
                "map" => {
                    if !words.is_empty() {
                        return Err(Fail::usage("map takes no argument besides --budget"));
                    }
                    Ok(Command::Map { budget })
                }
                _ if joined.trim().is_empty() => Err(Fail::usage(format!("{name} needs a text"))),
                "query" => Ok(Command::Query { question: joined, budget }),
                _ => Ok(Command::Pack { task: joined, budget }),
            }
        }
        "find" => {
            let mut words = Vec::new();
            let mut k = DEFAULT_FIND_K;
            let (mut only_words_mode, mut only_meaning) = (false, false);
            while let Some(item) = next_item(parser, global)? {
                match item {
                    Item::Word(w) => words.push(w),
                    Item::Flag(f) if f == "-k" => k = number(parser, "-k", 1)?,
                    Item::Flag(f) if f == "--words" => only_words_mode = true,
                    Item::Flag(f) if f == "--meaning" => only_meaning = true,
                    Item::Help => return Ok(Command::Help),
                    Item::Version => return Ok(Command::Version),
                    Item::Flag(f) => return Err(unknown(&f)),
                }
            }
            if only_words_mode && only_meaning {
                return Err(Fail::usage("find takes --words or --meaning, not both"));
            }
            let query = words.join(" ");
            if query.trim().is_empty() {
                return Err(Fail::usage("find needs some words to look for"));
            }
            let mode = if only_words_mode {
                Mode::Words
            } else if only_meaning {
                Mode::Meaning
            } else {
                Mode::Hybrid
            };
            Ok(Command::Find(FindArgs { query, k, mode }))
        }
        "retain" => parse_retain(parser, global),
        "recall" => parse_recall(parser, global),
        "reflect" => {
            let words = only_words!();
            let query = words.join(" ");
            if query.trim().is_empty() {
                return Err(Fail::usage("reflect needs some words"));
            }
            Ok(Command::Reflect { query })
        }
        "memory" => parse_memory(parser, global),
        "ask" => Ok(Command::Ask { questions: need_words("ask", only_words!())? }),
        other => Err(Fail::usage(format!("unknown command '{other}' (try: pgr help)"))),
    }
}

fn parse_retain(parser: &mut Parser, global: &mut Global) -> Res<Command> {
    let mut words = Vec::new();
    let mut args = RetainArgs { text: String::new(), kind: Kind::World, tags: Vec::new(), context: String::new(), when: None, whole: false };
    while let Some(item) = next_item(parser, global)? {
        match item {
            Item::Word(w) => words.push(w),
            Item::Flag(f) if f == "--kind" => {
                let value = text(parser, "--kind")?;
                args.kind = match Kind::from_name(&value) {
                    Some(k @ (Kind::World | Kind::Experience | Kind::Directive)) => k,
                    _ => return Err(Fail::usage(format!("--kind must be world, experience or directive, got '{value}'"))),
                };
            }
            Item::Flag(f) if f == "--tag" => args.tags.push(text(parser, "--tag")?),
            Item::Flag(f) if f == "--context" => args.context = text(parser, "--context")?,
            Item::Flag(f) if f == "--when" => args.when = Some(date(parser, "--when", false)?),
            Item::Flag(f) if f == "--whole" => args.whole = true,
            Item::Help => return Ok(Command::Help),
            Item::Version => return Ok(Command::Version),
            Item::Flag(f) => return Err(unknown(&f)),
        }
    }
    args.text = words.join(" ");
    if args.text.trim().is_empty() {
        return Err(Fail::usage("retain needs the text to keep"));
    }
    Ok(Command::Retain(args))
}

fn parse_recall(parser: &mut Parser, global: &mut Global) -> Res<Command> {
    let mut words = Vec::new();
    let mut args = RecallArgs::new(String::new());
    while let Some(item) = next_item(parser, global)? {
        match item {
            Item::Word(w) => words.push(w),
            Item::Flag(f) if f == "-k" => args.k = number(parser, "-k", 1)?,
            Item::Flag(f) if f == "--budget" => {
                let value = text(parser, "--budget")?;
                args.budget = match value.to_ascii_lowercase().as_str() {
                    "low" => Budget::Low,
                    "mid" => Budget::Mid,
                    "high" => Budget::High,
                    _ => return Err(Fail::usage(format!("--budget must be low, mid or high, got '{value}'"))),
                };
            }
            Item::Flag(f) if f == "--max-tokens" => args.max_tokens = number(parser, "--max-tokens", 1)?,
            Item::Flag(f) if f == "--tag" => args.tags.push(text(parser, "--tag")?),
            Item::Flag(f) if f == "--since" => args.since = Some(date(parser, "--since", false)?),
            Item::Flag(f) if f == "--until" => args.until = Some(date(parser, "--until", true)?),
            Item::Help => return Ok(Command::Help),
            Item::Version => return Ok(Command::Version),
            Item::Flag(f) => return Err(unknown(&f)),
        }
    }
    args.query = words.join(" ");
    if args.query.trim().is_empty() {
        return Err(Fail::usage("recall needs some words"));
    }
    Ok(Command::Recall(args))
}

fn parse_memory(parser: &mut Parser, global: &mut Global) -> Res<Command> {
    let sub = match next_item(parser, global)? {
        Some(Item::Word(w)) => w,
        Some(Item::Help) => return Ok(Command::Help),
        Some(Item::Version) => return Ok(Command::Version),
        Some(Item::Flag(f)) => return Err(unknown(&f)),
        None => return Err(Fail::usage("memory needs list, forget ID, stats or path")),
    };
    match sub.as_str() {
        "list" => {
            let mut k = DEFAULT_LIST_K;
            while let Some(item) = next_item(parser, global)? {
                match item {
                    Item::Flag(f) if f == "-k" => k = number(parser, "-k", 1)?,
                    Item::Help => return Ok(Command::Help),
                    Item::Version => return Ok(Command::Version),
                    Item::Flag(f) => return Err(unknown(&f)),
                    Item::Word(w) => return Err(Fail::usage(format!("memory list takes no argument, got '{w}'"))),
                }
            }
            Ok(Command::Memory(MemoryCmd::List { k }))
        }
        "forget" | "stats" | "path" => {
            let mut words = Vec::new();
            while let Some(item) = next_item(parser, global)? {
                match item {
                    Item::Word(w) => words.push(w),
                    Item::Help => return Ok(Command::Help),
                    Item::Version => return Ok(Command::Version),
                    Item::Flag(f) => return Err(unknown(&f)),
                }
            }
            match sub.as_str() {
                "forget" => {
                    if words.len() != 1 {
                        return Err(Fail::usage("memory forget needs one note id"));
                    }
                    Ok(Command::Memory(MemoryCmd::Forget { id: words.remove(0) }))
                }
                other => {
                    if !words.is_empty() {
                        return Err(Fail::usage(format!("memory {other} takes no argument")));
                    }
                    Ok(Command::Memory(if other == "stats" { MemoryCmd::Stats } else { MemoryCmd::Path }))
                }
            }
        }
        other => Err(Fail::usage(format!("unknown memory command '{other}' (list, forget, stats, path)"))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parsed(args: &[&str]) -> Res<Cli> {
        parse(args.iter().map(|a| a.to_string()))
    }

    #[test]
    fn find_takes_words_k_and_mode() {
        match parsed(&["find", "token", "expiry", "-k", "3", "--words"]).unwrap().command {
            Command::Find(f) => {
                assert_eq!(f.query, "token expiry");
                assert_eq!(f.k, 3);
                assert_eq!(f.mode, Mode::Words);
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn global_flags_work_before_and_after_the_command() {
        let cli = parsed(&["--root", "x", "status"]).unwrap();
        assert_eq!(cli.global.root, Some(PathBuf::from("x")));
        let cli = parsed(&["status", "--engines", "graph"]).unwrap();
        assert_eq!(cli.global.engines.as_deref(), Some("graph"));
    }

    #[test]
    fn usage_errors_exit_with_two() {
        for args in [
            vec![],
            vec!["nope"],
            vec!["def"],
            vec!["find"],
            vec!["find", "x", "--words", "--meaning"],
            vec!["find", "x", "-k", "zero"],
            vec!["path", "a"],
            vec!["--bogus", "status"],
            vec!["recall", "x", "--budget", "huge"],
            vec!["retain", "x", "--kind", "observation"],
            vec!["memory"],
        ] {
            let fail = parsed(&args).err().unwrap_or_else(|| panic!("{args:?} should fail"));
            assert_eq!(fail.code(), 2, "{args:?}");
        }
    }

    #[test]
    fn the_engine_of_a_command_is_known() {
        assert_eq!(parsed(&["def", "x"]).unwrap().command.engine(), Some(Engine::Graph));
        assert_eq!(parsed(&["find", "x"]).unwrap().command.engine(), Some(Engine::Search));
        assert_eq!(parsed(&["recall", "x"]).unwrap().command.engine(), Some(Engine::Memory));
        assert_eq!(parsed(&["status"]).unwrap().command.engine(), None);
    }
}
