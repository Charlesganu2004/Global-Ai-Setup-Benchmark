//! Writes to the memory bank from several processes at once, a server that keeps running
//! while other processes write, and a write that cannot be made durable.

mod common;
use common::{expect, Env};
use serde_json::{json, Value};
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, ChildStdout, Stdio};

/// A running `pgr mcp`, asked one call at a time.
struct Server {
    child: Child,
    input: Option<ChildStdin>,
    output: BufReader<ChildStdout>,
    next: u64,
}

impl Server {
    fn start(env: &Env) -> Server {
        let mut cmd = env.command();
        cmd.arg("--root").arg(&env.proj).arg("mcp");
        let mut child = cmd.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn().unwrap();
        let input = child.stdin.take();
        let output = BufReader::new(child.stdout.take().unwrap());
        Server { child, input, output, next: 1 }
    }

    /// Calls a tool and gives the text it answers.
    fn call(&mut self, tool: &str, arguments: Value) -> String {
        let request = json!({"jsonrpc": "2.0", "id": self.next, "method": "tools/call", "params": {"name": tool, "arguments": arguments}});
        self.next += 1;
        let input = self.input.as_mut().unwrap();
        writeln!(input, "{request}").unwrap();
        input.flush().unwrap();
        let mut line = String::new();
        self.output.read_line(&mut line).unwrap();
        let answer: Value = serde_json::from_str(&line).unwrap_or_else(|e| panic!("not JSON ({e}): {line}"));
        answer["result"]["content"][0]["text"].as_str().unwrap_or_else(|| panic!("no text in {answer}")).to_string()
    }

    fn stop(mut self) {
        drop(self.input.take());
        assert!(self.child.wait().unwrap().success());
    }
}

#[test]
fn a_server_reads_the_notes_other_processes_wrote_since_it_last_looked() {
    let env = Env::with_project();
    assert_eq!(env.run(&["refresh"]).code, 0);
    let mut server = Server::start(&env);

    assert_eq!(server.call("retain", json!({"text": "The deploy script lives in tools/deploy_alpha.sh and takes one argument."})), "kept n000001\n");
    // Another process writes while the server keeps running.
    expect(&env.run(&["retain", "Billing totals are rounded half to even in the ledger module.", "--whole"]), "kept n000002\n");

    // The server reads the bank as it is now, not as it was when it last looked.
    let recalled = server.call("recall", json!({"query": "billing totals rounded ledger"}));
    assert!(recalled.contains("#n000002"), "the note another process wrote is missing: {recalled}");
    server.stop();
}

#[test]
fn a_server_never_reuses_the_number_of_a_note_another_process_wrote() {
    let env = Env::with_project();
    assert_eq!(env.run(&["refresh"]).code, 0);
    let mut server = Server::start(&env);

    assert_eq!(server.call("retain", json!({"text": "The deploy script lives in tools/deploy_alpha.sh and takes one argument."})), "kept n000001\n");
    expect(&env.run(&["retain", "Billing totals are rounded half to even in the ledger module.", "--whole"]), "kept n000002\n");

    // Its own next note gets a new number and replaces nothing.
    assert_eq!(server.call("retain", json!({"text": "Cache entries expire after nine minutes in the session store."})), "kept n000003\n");
    // The same when the other process forgets a note the server still remembers.
    expect(&env.run(&["memory", "forget", "n000001"]), "forgot n000001\n");
    assert_eq!(server.call("retain", json!({"text": "Retry budgets reset at midnight in the billing region."})), "kept n000004\n");
    server.stop();

    let list = env.run(&["memory", "list"]);
    for id in ["#n000002", "#n000003", "#n000004"] {
        assert!(list.out.contains(id), "{id} is lost: {}", list.out);
    }
    assert!(!list.out.contains("#n000001"), "a forgotten note came back: {}", list.out);
    assert_eq!(list.out.lines().count(), 3, "{}", list.out);
}

#[test]
fn several_processes_keeping_notes_at_once_lose_none() {
    let env = Env::with_project();
    assert_eq!(env.run(&["refresh"]).code, 0);
    let texts = [
        "Alpha service retries three times with jitter before it gives up.",
        "Beta queue drains in strict arrival order during maintenance.",
        "Gamma exporter writes one parquet file per calendar month.",
        "Delta scheduler skips holidays listed in the regional calendar.",
        "Epsilon gateway signs every outgoing request with a rotating key.",
        "Zeta indexer rebuilds its thesaurus whenever the glossary changes.",
    ];
    let children: Vec<Child> = texts
        .iter()
        .map(|text| {
            let mut cmd = env.command();
            cmd.arg("--root").arg(&env.proj).args(["retain", text, "--whole"]);
            cmd.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap()
        })
        .collect();
    let mut ids = Vec::new();
    for child in children {
        let out = child.wait_with_output().unwrap();
        let err = String::from_utf8_lossy(&out.stderr).into_owned();
        assert_eq!(out.status.code(), Some(0), "{err}");
        let text = String::from_utf8_lossy(&out.stdout).into_owned();
        let id = text.strip_prefix("kept ").unwrap_or_else(|| panic!("{text} {err}")).trim().to_string();
        ids.push(id);
    }
    ids.sort();
    ids.dedup();
    assert_eq!(ids.len(), texts.len(), "two notes were given one number: {ids:?}");

    let stats = env.run(&["memory", "stats"]);
    assert!(stats.out.starts_with("6 notes"), "{}", stats.out);
    let list = env.run(&["memory", "list", "-k", "20"]);
    assert_eq!(list.out.lines().count(), 6, "{}", list.out);
}

/// The warning that a cache file could not be written, as `pgr` words it.
const CACHE_WARNING: &str = "pgr: warning: the cache files of the memory bank could not be written";

/// Makes the notes file of the bank read-only, so that nothing can be appended to it. Gives the
/// path to put right again, or `None` when this account can write to a read-only file anyway.
fn make_log_read_only(env: &Env) -> Option<std::path::PathBuf> {
    let log = env.index_dir().join("memory").join("notes.jsonl");
    let mut permissions = fs::metadata(&log).unwrap().permissions();
    permissions.set_readonly(true);
    fs::set_permissions(&log, permissions.clone()).unwrap();
    if fs::OpenOptions::new().append(true).open(&log).is_ok() {
        eprintln!("skipped: this account can write to a read-only file");
        permissions.set_readonly(false);
        fs::set_permissions(&log, permissions).unwrap();
        return None;
    }
    Some(log)
}

fn make_log_writable(log: &std::path::Path) {
    let mut permissions = fs::metadata(log).unwrap().permissions();
    permissions.set_readonly(false);
    fs::set_permissions(log, permissions).unwrap();
}

#[test]
fn a_note_whose_caches_cannot_be_written_is_kept_with_a_warning() {
    let env = Env::with_project();
    assert_eq!(env.run(&["refresh"]).code, 0);
    // The cache file cannot be written because a folder has its name.
    fs::create_dir_all(env.index_dir().join("memory").join("index.bin")).unwrap();
    let run = env.run(&["retain", "This note is appended but its caches cannot be written.", "--whole"]);
    // The note is safe in the log, so this is not a failure.
    assert_eq!(run.code, 0, "stderr was: {}", run.err);
    assert_eq!(run.out, "kept n000001\n");
    assert_eq!(run.err.matches(CACHE_WARNING).count(), 1, "one warning, said once: {}", run.err);
    assert!(run.err.contains("the change is safe in the log"), "{}", run.err);
    assert!(!run.err.contains("pgr: cannot write"), "{}", run.err);
    // The note is in the bank.
    let list = env.run(&["memory", "list"]);
    assert!(list.out.contains("This note is appended but its caches cannot be written.") && list.out.contains("#n000001"), "{}", list.out);
    assert_eq!(list.code, 0);
}

#[test]
fn a_note_that_cannot_be_appended_to_the_log_is_an_error_and_not_kept() {
    let env = Env::with_project();
    assert_eq!(env.run(&["refresh"]).code, 0);
    expect(&env.run(&["retain", "The first note reaches the log.", "--whole"]), "kept n000001\n");
    let Some(log) = make_log_read_only(&env) else { return };
    let run = env.run(&["retain", "This note cannot be appended to the log.", "--whole"]);
    make_log_writable(&log);
    assert_eq!(run.code, 1, "stdout was: {}", run.out);
    assert!(!run.out.contains("kept"), "{}", run.out);
    assert!(run.err.contains("pgr: cannot write the memory bank"), "{}", run.err);
    // Nothing was kept.
    let list = env.run(&["memory", "list"]);
    assert!(!list.out.contains("cannot be appended"), "{}", list.out);
    assert_eq!(list.out.lines().count(), 1, "{}", list.out);
}

#[test]
fn forgetting_a_note_whose_caches_cannot_be_written_works_with_a_warning() {
    let env = Env::with_project();
    assert_eq!(env.run(&["refresh"]).code, 0);
    expect(&env.run(&["retain", "A note that will be forgotten soon.", "--whole"]), "kept n000001\n");
    let blocker = env.index_dir().join("memory").join("index.bin");
    let _ = fs::remove_file(&blocker);
    fs::create_dir_all(&blocker).unwrap();
    let run = env.run(&["memory", "forget", "n000001"]);
    assert_eq!(run.code, 0, "stderr was: {}", run.err);
    assert_eq!(run.out, "forgot n000001\n");
    assert_eq!(run.err.matches(CACHE_WARNING).count(), 1, "{}", run.err);
    // The tombstone is in the log: the note is gone.
    expect(&env.run(&["memory", "list"]), "no notes\n");
}

#[test]
fn forgetting_a_note_that_cannot_be_appended_to_the_log_is_an_error() {
    let env = Env::with_project();
    assert_eq!(env.run(&["refresh"]).code, 0);
    expect(&env.run(&["retain", "A note that will be forgotten soon.", "--whole"]), "kept n000001\n");
    let Some(log) = make_log_read_only(&env) else { return };
    let run = env.run(&["memory", "forget", "n000001"]);
    make_log_writable(&log);
    assert_eq!(run.code, 1, "stdout was: {}", run.out);
    assert!(!run.out.contains("forgot"), "{}", run.out);
    assert!(run.err.contains("pgr: cannot write the memory bank"), "{}", run.err);
    // The note is still there.
    assert!(env.run(&["memory", "list"]).out.contains("#n000001"));
}

#[test]
fn a_server_that_cannot_write_a_cache_still_keeps_the_note() {
    let env = Env::with_project();
    assert_eq!(env.run(&["refresh"]).code, 0);
    fs::create_dir_all(env.index_dir().join("memory")).unwrap();
    fs::create_dir_all(env.index_dir().join("memory").join("index.bin")).unwrap();
    let mut server = Server::start(&env);
    assert_eq!(server.call("retain", json!({"text": "The server keeps this note although its caches cannot be written."})), "kept n000001\n");
    let recalled = server.call("recall", json!({"query": "server keeps note caches"}));
    assert!(recalled.contains("#n000001"), "{recalled}");
    server.stop();
    assert!(env.run(&["memory", "list"]).out.contains("#n000001"));
}

#[test]
fn a_server_that_holds_the_bank_sees_a_forget_and_a_new_note_between_two_calls() {
    let env = Env::with_project();
    assert_eq!(env.run(&["refresh"]).code, 0);
    expect(&env.run(&["retain", "The nightly export runs at three in the morning.", "--whole"]), "kept n000001\n");
    let mut server = Server::start(&env);
    let first = server.call("recall", json!({"query": "nightly export"}));
    assert!(first.contains("#n000001"), "{first}");

    // Another process forgets that note and keeps another, while the server holds its bank.
    expect(&env.run(&["memory", "forget", "n000001"]), "forgot n000001\n");
    expect(&env.run(&["retain", "Invoices are archived after ninety days.", "--whole"]), "kept n000002\n");
    let second = server.call("recall", json!({"query": "nightly export invoices archived"}));
    assert!(!second.contains("#n000001"), "the forgotten note is still answered: {second}");
    assert!(second.contains("#n000002"), "{second}");

    // A third look finds nothing new and answers with the same notes.
    let third = server.call("recall", json!({"query": "nightly export invoices archived"}));
    assert!(third.contains("#n000002") && !third.contains("#n000001"), "{third}");
    server.stop();
}

#[test]
fn notes_that_share_their_sources_make_one_observation_that_lists_both_subjects() {
    let env = Env::with_project();
    assert_eq!(env.run(&["refresh"]).code, 0);
    for text in [
        "config_loader.py defines load_config for reading settings.",
        "config_loader.py exposes load_config to the command line.",
        "load_config in config_loader.py falls back to defaults.",
    ] {
        let run = env.run(&["retain", text, "--whole"]);
        assert_eq!(run.code, 0, "{}", run.err);
    }
    let reflected = env.run(&["reflect", "config_loader"]);
    assert_eq!(reflected.code, 0, "{}", reflected.err);
    let observations: Vec<&str> = reflected.out.lines().skip_while(|l| *l != "observations:").skip(1).take_while(|l| l.starts_with("- ")).collect();
    assert_eq!(observations.len(), 1, "one observation, not one copy per subject:\n{}", reflected.out);
    assert!(observations[0].starts_with("- config_loader.py, load_config: config_loader.py defines load_config"), "{}", observations[0]);
    // Both subjects lead to it.
    let id = observations[0].rsplit_once('#').unwrap().1.to_string();
    for subject in ["config_loader.py", "load_config"] {
        let line = reflected.out.lines().find(|l| l.trim_start().starts_with(&format!("{subject}:"))).unwrap_or_else(|| panic!("{subject} is no subject:\n{}", reflected.out));
        assert!(line.contains(&format!("#{id}")), "{line}");
    }
    let stats = env.run(&["memory", "stats"]);
    assert!(stats.out.starts_with("4 notes (world 3, experience 0, observation 1, directive 0)"), "{}", stats.out);
}

#[test]
fn today_in_a_note_is_the_day_of_the_machines_time_zone() {
    let Some(offset) = common::machine_offset() else {
        eprintln!("skipped: the machine's time zone cannot be read");
        return;
    };
    let env = Env::with_project();
    assert_eq!(env.run(&["refresh"]).code, 0);
    let clock = || std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs() as i64;
    let before = clock();
    expect(&env.run(&["retain", "The cache was cleared today.", "--whole"]), "kept n000001\n");
    let after = clock();
    let log = fs::read_to_string(env.index_dir().join("memory").join("notes.jsonl")).unwrap();
    let note: Value = serde_json::from_str(log.lines().next().unwrap()).unwrap();
    let (start, end) = (note["occurred_start"].as_i64().unwrap(), note["occurred_end"].as_i64().unwrap());
    let local_midnight = |t: i64| (t + offset).div_euclid(86_400) * 86_400 - offset;
    assert!(start == local_midnight(before) || start == local_midnight(after), "start {start}, offset {offset}, clock {before}..{after}");
    assert_eq!(end, start + 86_399);
}