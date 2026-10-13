//! `dump ROOT [--json]`: extracts every source file under ROOT and prints what it found.
//!
//! The plain output has one line per file, `path lang grade symbols calls imports`,
//! then the totals by grade and the elapsed milliseconds of the walk and extraction.
//! With `--json` it prints one JSON document instead and writes the totals to stderr.
//!
//! Setting `LXR_FORCE_LEXICAL=1` sends every file through the token-based extractor,
//! also the languages that have a full parser. The extractor is compiled into this
//! example from the crate's own source files, because the crate keeps it private.

#![allow(dead_code)]

#[path = "../src/common.rs"]
mod common;
#[path = "../src/lexical.rs"]
mod lexical;

use lxr_core::{FileFacts, ParseGrade};
use lxr_extract::{lang_of, walk, WalkOpts};
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;
use std::time::Instant;

/// Stack for each worker: the parsers recurse, and a deep file should not end the run.
const WORKER_STACK: usize = 64 * 1024 * 1024;

fn main() {
    let mut root: Option<String> = None;
    let mut json = false;
    for arg in std::env::args().skip(1) {
        if arg == "--json" {
            json = true;
        } else if root.is_none() {
            root = Some(arg);
        }
    }
    let Some(root) = root else {
        eprintln!("usage: dump ROOT [--json]");
        std::process::exit(2);
    };
    let root = PathBuf::from(root);
    let force = std::env::var_os("LXR_FORCE_LEXICAL").map_or(false, |v| !v.is_empty() && v != "0");

    let started = Instant::now();
    let paths = walk(&root, &WalkOpts::default());
    let walked = started.elapsed();
    let facts = extract_all(&root, &paths, force);
    let elapsed = started.elapsed();

    let ast = facts.iter().filter(|f| f.grade == ParseGrade::Ast).count();
    let lexical = facts.len() - ast;
    let totals = format!(
        "totals: files {} ast {} lexical {} symbols {} calls {} imports {}\nelapsed ms: {:.1} (walk {:.1})",
        facts.len(),
        ast,
        lexical,
        facts.iter().map(|f| f.symbols.len()).sum::<usize>(),
        facts.iter().map(|f| f.calls.len()).sum::<usize>(),
        facts.iter().map(|f| f.imports.len()).sum::<usize>(),
        elapsed.as_secs_f64() * 1000.0,
        walked.as_secs_f64() * 1000.0
    );
    let stdout = std::io::stdout();
    let mut out = BufWriter::new(stdout.lock());
    let written = if json { write_json(&mut out, &facts) } else { write_plain(&mut out, &facts) };
    if let Err(error) = written.and_then(|_| out.flush()) {
        eprintln!("could not write the result: {error}");
        std::process::exit(1);
    }
    if json {
        eprintln!("{totals}");
    } else if writeln!(out, "{totals}").and_then(|_| out.flush()).is_err() {
        std::process::exit(1);
    }
}

fn extract_one(root: &Path, path: &str, force: bool) -> Option<FileFacts> {
    if !force {
        return lxr_extract::extract_path(root, path);
    }
    let lang = lang_of(Path::new(path))?;
    let bytes = std::fs::read(root.join(path)).ok()?;
    let text = String::from_utf8_lossy(&bytes);
    let parts = lexical::parse(&text, lang);
    Some(common::finish(path, lang, &text, ParseGrade::Lexical, parts))
}

/// Extracts the files on as many threads as the machine has, in the order of `paths`.
fn extract_all(root: &Path, paths: &[String], force: bool) -> Vec<FileFacts> {
    let workers = std::thread::available_parallelism().map_or(1, |n| n.get()).min(paths.len().max(1));
    let next = AtomicUsize::new(0);
    let results: Mutex<Vec<(usize, FileFacts)>> = Mutex::new(Vec::new());
    std::thread::scope(|scope| {
        for _ in 0..workers {
            let worker = || {
                let mut local = Vec::new();
                loop {
                    let i = next.fetch_add(1, Ordering::Relaxed);
                    if i >= paths.len() {
                        break;
                    }
                    if let Some(facts) = extract_one(root, &paths[i], force) {
                        local.push((i, facts));
                    }
                }
                if let Ok(mut all) = results.lock() {
                    all.extend(local);
                }
            };
            let spawned = std::thread::Builder::new().stack_size(WORKER_STACK).spawn_scoped(scope, worker);
            if spawned.is_err() {
                eprintln!("could not start a worker thread");
            }
        }
    });
    let mut all = results.into_inner().unwrap_or_default();
    all.sort_by_key(|(i, _)| *i);
    all.into_iter().map(|(_, facts)| facts).collect()
}

fn grade_word(grade: ParseGrade) -> &'static str {
    match grade {
        ParseGrade::Ast => "Ast",
        ParseGrade::Lexical => "Lexical",
    }
}

fn write_plain(out: &mut impl Write, facts: &[FileFacts]) -> std::io::Result<()> {
    for f in facts {
        writeln!(
            out,
            "{} {} {} {} {} {}",
            f.path,
            f.lang.name(),
            grade_word(f.grade),
            f.symbols.len(),
            f.calls.len(),
            f.imports.len()
        )?;
    }
    Ok(())
}

/// `text` as a JSON string literal.
fn json_string(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn write_json(out: &mut impl Write, facts: &[FileFacts]) -> std::io::Result<()> {
    write!(out, "{{\"files\":[")?;
    for (n, f) in facts.iter().enumerate() {
        if n > 0 {
            write!(out, ",")?;
        }
        write!(out, "{{\"path\":{},\"grade\":{},\"defs\":[", json_string(&f.path), json_string(grade_word(f.grade)))?;
        for (i, s) in f.symbols.iter().enumerate() {
            if i > 0 {
                write!(out, ",")?;
            }
            write!(out, "[{},{},{},{}]", json_string(&s.name), json_string(s.kind.word()), s.start, s.end)?;
        }
        write!(out, "],\"calls\":[")?;
        for (i, c) in f.calls.iter().enumerate() {
            if i > 0 {
                write!(out, ",")?;
            }
            let caller = c.caller.and_then(|k| f.symbols.get(k as usize)).map_or("<module>", |s| s.name.as_str());
            write!(out, "[{},{},{}]", json_string(caller), json_string(&c.name), c.line)?;
        }
        write!(out, "],\"imports\":[")?;
        for (i, m) in f.imports.iter().enumerate() {
            if i > 0 {
                write!(out, ",")?;
            }
            write!(
                out,
                "[{},{},{},{},{}]",
                json_string(&m.module),
                json_string(&m.name),
                json_string(&m.alias),
                m.level,
                m.line
            )?;
        }
        write!(out, "]}}")?;
    }
    writeln!(out, "]}}")
}
