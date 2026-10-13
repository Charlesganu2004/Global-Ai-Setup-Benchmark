//! Builds a search index over a folder and answers queries from it.
//!
//! ```text
//! searchdemo [--mode hybrid|words|meaning] [--limit N] [--index DIR] [--rebuild]
//!            [--queries FILE] [--repeat N] [--timing] ROOT [QUERY...]
//! ```
//!
//! Walks ROOT, extracts and chunks every source file with rayon, builds the index (in DIR
//! when given and kept for the next run, otherwise in a temporary folder) with the model
//! that `Embedder::locate` finds, and prints for each query its top hits as
//! `symbol path:start-end score`, under a `== QUERY` line. A hit that lies in no
//! definition has `-` as its symbol. Figures go to stderr, each line starting `# `.
//! `--queries FILE` reads one query per line. `--repeat N` runs each query N times and
//! reports the median time of one search. `--timing` also measures the embedding and
//! the word index on their own, which costs two more passes over the chunks.

use lxr_extract::{extract, lang_of, walk, WalkOpts};
use lxr_rag::{chunk_file, Chunk, Mode, SearchIndex, SearchOpts};
use lxr_text::Embedder;
use rayon::prelude::*;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

struct Args {
    mode: Mode,
    limit: usize,
    index: Option<PathBuf>,
    rebuild: bool,
    queries_file: Option<PathBuf>,
    repeat: usize,
    timing: bool,
    root: PathBuf,
    queries: Vec<String>,
}

fn parse_args() -> Result<Args, String> {
    let mut args = Args {
        mode: Mode::Hybrid,
        limit: 5,
        index: None,
        rebuild: false,
        queries_file: None,
        repeat: 1,
        timing: false,
        root: PathBuf::new(),
        queries: Vec::new(),
    };
    let mut positional: Vec<String> = Vec::new();
    let mut it = std::env::args().skip(1);
    while let Some(arg) = it.next() {
        let mut value = |name: &str| it.next().ok_or_else(|| format!("{name} needs a value"));
        match arg.as_str() {
            "--mode" => {
                args.mode = match value("--mode")?.as_str() {
                    "hybrid" => Mode::Hybrid,
                    "words" => Mode::Words,
                    "meaning" => Mode::Meaning,
                    other => return Err(format!("unknown mode {other:?}: use hybrid, words or meaning")),
                }
            }
            "--limit" => args.limit = value("--limit")?.parse().map_err(|_| "--limit needs a number".to_string())?,
            "--repeat" => args.repeat = value("--repeat")?.parse().map_err(|_| "--repeat needs a number".to_string())?,
            "--index" => args.index = Some(PathBuf::from(value("--index")?)),
            "--queries" => args.queries_file = Some(PathBuf::from(value("--queries")?)),
            "--rebuild" => args.rebuild = true,
            "--timing" => args.timing = true,
            _ => positional.push(arg),
        }
    }
    let mut positional = positional.into_iter();
    args.root = PathBuf::from(positional.next().ok_or("usage: searchdemo [options] ROOT [QUERY...]")?);
    args.queries.extend(positional);
    if let Some(file) = &args.queries_file {
        let text = fs::read_to_string(file).map_err(|e| format!("{}: {e}", file.display()))?;
        args.queries.extend(text.lines().map(str::trim).filter(|l| !l.is_empty()).map(str::to_string));
    }
    args.repeat = args.repeat.max(1);
    Ok(args)
}

fn ms(d: Duration) -> f64 {
    d.as_secs_f64() * 1000.0
}

fn folder_bytes(dir: &Path) -> u64 {
    fs::read_dir(dir)
        .map(|entries| entries.flatten().filter_map(|e| e.metadata().ok()).filter(|m| m.is_file()).map(|m| m.len()).sum())
        .unwrap_or(0)
}

/// Reads, extracts and chunks every file of `root`. Returns the chunks, the number of
/// files, and the time spent in `chunk_file` alone, summed over the threads.
fn chunk_all(root: &Path) -> (Vec<Chunk>, usize, Duration) {
    let files = walk(root, &WalkOpts::default());
    let chunk_nanos = AtomicU64::new(0);
    let chunks: Vec<Chunk> = files
        .par_iter()
        .flat_map_iter(|path| {
            let Some(lang) = lang_of(Path::new(path)) else { return Vec::new() };
            let Ok(bytes) = fs::read(root.join(path)) else { return Vec::new() };
            let text = String::from_utf8_lossy(&bytes);
            let facts = extract(path, lang, &text);
            let started = Instant::now();
            let chunks = chunk_file(&facts, &text);
            chunk_nanos.fetch_add(started.elapsed().as_nanos() as u64, Ordering::Relaxed);
            chunks
        })
        .collect();
    (chunks, files.len(), Duration::from_nanos(chunk_nanos.load(Ordering::Relaxed)))
}

fn run() -> Result<(), String> {
    let args = parse_args()?;
    let embedder = match Embedder::locate() {
        Some(dir) => {
            let started = Instant::now();
            let embedder = Embedder::open(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
            eprintln!("# model {} opened in {:.1} ms", dir.display(), ms(started.elapsed()));
            Some(embedder)
        }
        None => {
            eprintln!("# no embedding model found: Hybrid will use words alone");
            None
        }
    };

    let temporary = std::env::temp_dir().join(format!("searchdemo-{}", std::process::id()));
    let dir = args.index.clone().unwrap_or_else(|| temporary.join("index"));
    let reuse = args.index.is_some() && !args.rebuild;
    let opened = if reuse {
        let started = Instant::now();
        match SearchIndex::open(&dir) {
            Ok(index) => {
                eprintln!("# index opened from {} in {:.1} ms", dir.display(), ms(started.elapsed()));
                Some(index)
            }
            Err(error) => {
                eprintln!("# {}: {error}; building it", dir.display());
                None
            }
        }
    } else {
        None
    };

    let index = match opened {
        Some(index) => index,
        None => {
            let started = Instant::now();
            let (chunks, files, in_chunker) = chunk_all(&args.root);
            let chunk_time = started.elapsed();
            let bytes: usize = chunks.iter().map(|c| c.text.len()).sum();
            eprintln!(
                "# files {files}, chunks {}, chunk text {:.1} MB; read+extract+chunk {:.0} ms (chunk_file alone {:.0} ms of thread time)",
                chunks.len(),
                bytes as f64 / 1e6,
                ms(chunk_time),
                ms(in_chunker)
            );
            if args.timing {
                if let Some(e) = &embedder {
                    let started = Instant::now();
                    for batch in chunks.chunks(2048) {
                        let texts: Vec<&str> = batch.iter().map(|c| c.text.as_str()).collect();
                        std::hint::black_box(e.embed_many(&texts));
                    }
                    eprintln!("# embed {} chunks alone: {:.0} ms", chunks.len(), ms(started.elapsed()));
                }
                let words_dir = temporary.join("words-only");
                let started = Instant::now();
                SearchIndex::build(chunks.clone(), None, &words_dir).map_err(|e| e.to_string())?;
                eprintln!("# word index and table built and written: {:.0} ms", ms(started.elapsed()));
                let _ = fs::remove_dir_all(&words_dir);
            }
            let started = Instant::now();
            let index = SearchIndex::build(chunks, embedder.as_ref(), &dir).map_err(|e| format!("{}: {e}", dir.display()))?;
            eprintln!(
                "# full build {:.0} ms; index {} chunks, vectors {}, {:.2} MB on disk in {}",
                ms(started.elapsed()),
                index.len(),
                index.has_vectors(),
                folder_bytes(&dir) as f64 / 1e6,
                dir.display()
            );
            index
        }
    };

    let opts = SearchOpts { limit: args.limit, mode: args.mode, alpha: None };
    let mut medians: Vec<f64> = Vec::new();
    let total = Instant::now();
    for query in &args.queries {
        let mut times: Vec<f64> = Vec::new();
        let mut hits = Vec::new();
        for _ in 0..args.repeat {
            let started = Instant::now();
            hits = index.search(query, embedder.as_ref(), &opts);
            times.push(ms(started.elapsed()));
        }
        times.sort_by(f64::total_cmp);
        medians.push(times[times.len() / 2]);
        println!("== {query}");
        for hit in &hits {
            let symbol = if hit.symbol.is_empty() { "-" } else { hit.symbol.as_str() };
            println!("{symbol} {}:{}-{} {:.5}", hit.path, hit.start, hit.end, hit.score);
        }
    }
    if !medians.is_empty() {
        let all = ms(total.elapsed());
        medians.sort_by(f64::total_cmp);
        eprintln!(
            "# {} queries, {:?}, limit {}: {:.1} ms in all; one search, median over {} runs: {:.3} ms (median of queries), {:.3} ms (slowest query)",
            medians.len(),
            args.mode,
            args.limit,
            all,
            args.repeat,
            medians[medians.len() / 2],
            medians[medians.len() - 1]
        );
    }
    let _ = fs::remove_dir_all(&temporary);
    Ok(())
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("searchdemo: {message}");
            ExitCode::FAILURE
        }
    }
}
