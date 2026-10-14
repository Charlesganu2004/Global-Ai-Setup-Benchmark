//! The team hub of version 0.2: the global flags `--bank`, `--task` and `--from`, `pgr handoff`,
//! `pgr loadout`, the changed `pgr pack` with its names section and its budget rule, the MCP tools
//! and the help text.

mod common;
use common::{expect, Env, Run};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::process::Stdio;

const RULE: &str = "Always run the formatter before committing.";

/// Replaces every age (`3m ago`) by `<age> ago`, so that two runs compare byte for byte.
fn no_age(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut at = 0;
    while at < text.len() {
        if text[at..].starts_with(" ago") {
            let bytes = out.as_bytes();
            if let Some(&unit) = bytes.last() {
                if matches!(unit, b's' | b'm' | b'h' | b'd') {
                    let mut start = bytes.len() - 1;
                    while start > 0 && bytes[start - 1].is_ascii_digit() {
                        start -= 1;
                    }
                    if start < bytes.len() - 1 {
                        out.truncate(start);
                        out.push_str("<age>");
                    }
                }
            }
            out.push_str(" ago");
            at += 4;
            continue;
        }
        let c = text[at..].chars().next().unwrap();
        out.push(c);
        at += c.len_utf8();
    }
    out
}

/// The tokens a line costs in a budget: its estimate and its line break.
fn cost(line: &str) -> usize {
    (line.len() + 3) / 4 + 1
}

/// The tokens a line costs once the age in it is put back (a run takes a second or two).
fn cost_of_aged(line: &str) -> usize {
    cost(&line.replace("<age>", "0s"))
}

/// A project with its index built.
fn project() -> Env {
    let env = Env::with_project();
    assert_eq!(env.run(&["refresh"]).code, 0);
    env
}

/// Files a handoff and checks that it was accepted.
fn handoff(env: &Env, task: &str, from: &str, text: &str) -> String {
    let run = env.run(&["--task", task, "--from", from, "handoff", text]);
    assert_eq!(run.code, 0, "{}", run.err);
    run.out
}

/// Keeps a note with `retain` and checks that it was accepted.
fn keep(env: &Env, text: &str) {
    let run = env.run(&["retain", text]);
    assert_eq!(run.code, 0, "{}", run.err);
}

/// What `loadout --task TASK ...` prints, with the ages replaced.
fn loadout(env: &Env, args: &[&str]) -> String {
    let run = env.run(args);
    assert_eq!(run.code, 0, "{}", run.err);
    no_age(&run.out)
}

/// The folder of the bank of the project.
fn bank_dir(env: &Env) -> PathBuf {
    PathBuf::from(env.run(&["memory", "path"]).out.trim())
}

/// Every file of a folder: its bytes and its modification time.
fn folder_state(dir: &PathBuf) -> BTreeMap<String, (Vec<u8>, std::time::SystemTime)> {
    fs::read_dir(dir)
        .unwrap()
        .filter_map(Result::ok)
        .filter(|e| e.path().is_file())
        .map(|e| (e.file_name().to_string_lossy().into_owned(), (fs::read(e.path()).unwrap(), e.metadata().unwrap().modified().unwrap())))
        .collect()
}

/// The names of the files that differ between two states of a folder.
fn differing(before: &BTreeMap<String, (Vec<u8>, std::time::SystemTime)>, after: &BTreeMap<String, (Vec<u8>, std::time::SystemTime)>) -> Vec<String> {
    let mut names: Vec<String> = before.keys().chain(after.keys()).filter(|k| before.get(*k) != after.get(*k)).cloned().collect();
    names.sort();
    names.dedup();
    names
}

/// The lines of a loadout's layer, from its title to the line before the next title or the closing line.
fn layer<'a>(text: &'a str, title: &str) -> Vec<&'a str> {
    let mut lines = text.lines().skip_while(|l| *l != title);
    match lines.next() {
        Some(first) => std::iter::once(first).chain(lines.take_while(|l| !l.starts_with("== ") && !l.starts_with("loadout:"))).collect(),
        None => Vec::new(),
    }
}

/// The ids at the end of the note lines of a layer, in order.
fn ids(lines: &[&str]) -> Vec<String> {
    lines.iter().filter(|l| l.starts_with("- ")).map(|l| l.rsplit('#').next().unwrap().to_string()).collect()
}

/// The notes of the bank as JSON objects, the last state of each id.
fn notes_in(env: &Env) -> BTreeMap<String, Value> {
    let log = fs::read_to_string(bank_dir(env).join("notes.jsonl")).unwrap();
    let mut notes = BTreeMap::new();
    for line in log.lines() {
        let value: Value = serde_json::from_str(line).unwrap();
        if let Some(id) = value["id"].as_str() {
            notes.insert(id.to_string(), value.clone());
        }
    }
    notes
}

fn tags_of(note: &Value) -> Vec<String> {
    note["tags"].as_array().unwrap().iter().map(|t| t.as_str().unwrap().to_string()).collect()
}

// ---- names: --bank, --task, --from and their variables ----

#[test]
fn a_name_that_could_leave_its_folder_or_is_empty_is_a_usage_error_from_a_flag() {
    let env = project();
    for (flag, kind) in [("--bank", "bank"), ("--task", "task"), ("--from", "role")] {
        for value in ["..\\x", "../x", "a\\b", "a/b", "", ".", "..", "has space", "colon:name", &"n".repeat(65)] {
            let run = env.run(&[flag, value, "status"]);
            assert_eq!(run.code, 2, "{flag} {value:?}: {}", run.err);
            assert_eq!(run.out, "", "{flag} {value:?}");
            assert_eq!(run.err.trim_end(), format!("pgr: {value} is not a valid {kind} name"), "{flag} {value:?}");
        }
    }
    assert!(!env.home.join("banks").exists(), "no bank folder was made");
    assert!(!env.base.join("x").exists() && !env.home.join("x").exists());
}

#[test]
fn a_name_from_the_environment_is_checked_the_same_way() {
    let env = project();
    for (variable, kind, command) in [
        ("PGR_BANK", "bank", vec!["status"]),
        ("PGR_TASK", "task", vec!["loadout"]),
        ("PGR_ROLE", "role", vec!["--task", "t", "handoff", "a fact"]),
    ] {
        for value in ["..\\x", "a\\b", "../x", "a/b", "..", "has space"] {
            let run = env.run_with(&command, &[(variable, value)]);
            assert_eq!(run.code, 2, "{variable}={value:?}: {}", run.err);
            assert_eq!(run.out, "");
            assert_eq!(run.err.trim_end(), format!("pgr: {value} is not a valid {kind} name"), "{variable}={value:?}");
        }
    }
    // A name that fits the rule is accepted, 64 characters included.
    let long = "n".repeat(64);
    for value in ["team-1", "a.b_c", long.as_str()] {
        assert_eq!(env.run_with(&["status"], &[("PGR_BANK", value)]).code, 0, "{value}");
    }
    assert!(!env.home.join("banks").join("..").join("x").exists());
}

#[test]
fn the_bank_flag_wins_over_the_variable_and_names_a_folder_under_banks() {
    let env = project();
    let path = |run: Run| PathBuf::from(run.out.trim());
    let flagged = path(env.run_with(&["--bank", "one", "memory", "path"], &[("PGR_BANK", "two")]));
    assert!(flagged.ends_with(PathBuf::from("banks").join("one")), "{flagged:?}");
    let from_variable = path(env.run_with(&["memory", "path"], &[("PGR_BANK", "two")]));
    assert!(from_variable.ends_with(PathBuf::from("banks").join("two")), "{from_variable:?}");
    // The flag wins, so a bad variable is not even looked at.
    let run = env.run_with(&["--bank", "one", "memory", "path"], &[("PGR_BANK", "..\\x")]);
    assert_eq!(run.code, 0, "{}", run.err);
    // An empty variable is the same as none.
    let empty = path(env.run_with(&["memory", "path"], &[("PGR_BANK", "")]));
    assert!(!empty.to_string_lossy().contains("banks"), "{empty:?}");
}

#[test]
fn one_bank_is_shared_by_two_repositories() {
    let env = project();
    let other = env.base.join("proj2");
    fs::create_dir_all(&other).unwrap();
    fs::write(other.join("tool.py"), "def run():\n    \"\"\"Run it.\"\"\"\n    return 1\n").unwrap();
    let said = env.run(&["--bank", "team", "--task", "t1", "--from", "scout", "handoff", "The nightly job finishes before six o'clock."]);
    assert_eq!(said.code, 0, "{}", said.err);
    let seen = no_age(&env.run_at(&other, &["--bank", "team", "--task", "t1", "loadout"], &[]).out);
    assert!(seen.contains("== task t1\n- The nightly job finishes before six o'clock.  (<age> ago) [scout] #n000001\n"), "{seen}");
    // Without the flag the other repository has a bank of its own.
    expect(&env.run_at(&other, &["loadout"], &[]), "no notes\n");
}

// ---- handoff ----

#[test]
fn handoff_needs_a_task() {
    let env = project();
    let run = env.run(&["handoff", "a fact"]);
    assert_eq!(run.code, 2);
    assert_eq!(run.out, "");
    assert_eq!(run.err.trim_end(), "pgr: handoff needs --task NAME");
    assert!(!bank_dir(&env).join("notes.jsonl").exists(), "nothing was kept");
    // The task can come from the flag, from the variable, and the flag wins.
    expect(&env.run(&["--task", "t1", "handoff", "The exporter writes csv files."]), "kept n000001\nhandoff t1: 1 notes from agent, 0 name code\n");
    let by_variable = env.run_with(&["handoff", "The scheduler retries failed jobs."], &[("PGR_TASK", "t2"), ("PGR_ROLE", "scout")]);
    expect(&by_variable, "kept n000002\nhandoff t2: 1 notes from scout, 0 name code\n");
    let flag_wins = env.run_with(&["--task", "t3", "--from", "judge", "handoff", "Logs rotate at midnight."], &[("PGR_TASK", "t2"), ("PGR_ROLE", "scout")]);
    expect(&flag_wins, "kept n000003\nhandoff t3: 1 notes from judge, 0 name code\n");
}

#[test]
fn a_handoff_tags_every_note_with_the_task_and_the_role_and_gives_it_the_context() {
    let env = project();
    let out = handoff(&env, "t1", "scout", "load_config reads the settings file.\nThe exporter writes csv files.");
    assert_eq!(out, "kept n000001\nkept n000002\nhandoff t1: 2 notes from scout, 1 name code\n");
    let notes = notes_in(&env);
    for id in ["n000001", "n000002"] {
        let tags = tags_of(&notes[id]);
        assert!(tags.contains(&"task:t1".to_string()) && tags.contains(&"from:scout".to_string()), "{id}: {tags:?}");
        assert_eq!(notes[id]["context"], "handoff");
    }
    // The tags are the ones recall filters by.
    let recalled = env.run(&["recall", "settings", "--tag", "task:t1"]);
    assert!(recalled.out.contains("load_config reads the settings file") && recalled.out.contains("#n000001"), "{}", recalled.out);
    // The other options of retain are kept: the kind, the tags of the caller, one note with --whole.
    let whole = env.run(&["--task", "t1", "handoff", "--kind", "experience", "--tag", "extra", "--whole", "I tried it.", "It failed twice."]);
    expect(&whole, "kept n000003\nhandoff t1: 1 notes from agent, 0 name code\n");
    let note = &notes_in(&env)["n000003"];
    assert_eq!(note["kind"], "Experience");
    assert_eq!(note["text"], "I tried it. It failed twice.");
    assert!(tags_of(note).contains(&"extra".to_string()));
}

#[test]
fn a_handoff_that_repeats_a_fact_reinforces_it_and_adds_the_task_and_the_role() {
    let env = project();
    expect(&env.run(&["retain", "The exporter writes csv files."]), "kept n000001\n");
    let again = handoff(&env, "t1", "reviewer", "the exporter writes CSV files");
    assert_eq!(again, "reinforced n000001 (seen 2 times)\nhandoff t1: 1 notes from reviewer, 0 name code\n");
    let tags = tags_of(&notes_in(&env)["n000001"]);
    assert!(tags.contains(&"task:t1".to_string()) && tags.contains(&"from:reviewer".to_string()), "{tags:?}");
}

#[test]
fn a_handoff_reads_standard_input_up_to_the_limit() {
    let env = project();
    let run = env.run_stdin(&["--task", "t1", "handoff", "-"], "The exporter writes csv files.\nThe scheduler retries failed jobs.\n");
    expect(&run, "kept n000001\nkept n000002\nhandoff t1: 2 notes from agent, 0 name code\n");

    // Exactly 65,536 bytes is accepted, one more is not.
    let pad = |total: usize| {
        let mut text = String::from("Logs rotate at midnight.\n");
        text.push_str(&" ".repeat(total - text.len()));
        text
    };
    let at_limit = env.run_stdin(&["--task", "t1", "handoff", "-"], &pad(65_536));
    expect(&at_limit, "kept n000003\nhandoff t1: 1 notes from agent, 0 name code\n");
    let over = env.run_stdin(&["--task", "t1", "handoff", "-"], &pad(65_537));
    assert_eq!(over.code, 2, "{}", over.err);
    assert_eq!(over.out, "");
    assert!(over.err.contains("65536"), "{}", over.err);
    assert_eq!(notes_in(&env).len(), 3, "nothing from the long text was kept");

    let empty = env.run_stdin(&["--task", "t1", "handoff", "-"], "  \n");
    assert_eq!(empty.code, 2);
    assert_eq!(empty.err.trim_end(), "pgr: handoff needs the text to keep");
}

#[test]
fn text_on_standard_input_that_is_not_utf8_is_a_usage_error() {
    let env = project();
    let mut cmd = env.command();
    cmd.arg("--root").arg(&env.proj).args(["--task", "t1", "handoff", "-"]);
    let mut child = cmd.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
    child.stdin.take().unwrap().write_all(&[b'o', b'k', 0xff, 0xfe, b'\n']).unwrap();
    let done = child.wait_with_output().unwrap();
    assert_eq!(done.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&done.stderr).contains("UTF-8"), "{}", String::from_utf8_lossy(&done.stderr));
}

#[test]
fn handoff_and_loadout_belong_to_the_memory_engine() {
    let env = project();
    for args in [vec!["--task", "t", "handoff", "x"], vec!["loadout"]] {
        let mut full = vec!["--engines", "graph,search"];
        full.extend(&args);
        let run = env.run(&full);
        assert_eq!(run.code, 2, "{args:?}: {}", run.err);
        assert_eq!(run.err.trim_end(), "pgr: the memory engine is off");
    }
}

// ---- loadout ----

#[test]
fn a_loadout_without_a_bank_or_without_a_note_to_print_says_so_and_makes_nothing() {
    let env = project();
    expect(&env.run(&["loadout"]), "no notes\n");
    expect(&env.run(&["--task", "t1", "loadout", "anything"]), "no notes\n");
    assert!(!bank_dir(&env).exists(), "a loadout does not make the bank folder");
    // A bank whose notes are neither core nor of the task nor about the words prints nothing either.
    expect(&env.run(&["retain", "The exporter writes csv files."]), "kept n000001\n");
    expect(&env.run(&["loadout"]), "no notes\n");
    expect(&env.run(&["--task", "other", "loadout", "zzzqqq"]), "no notes\n");
}

#[test]
fn the_layers_come_in_order_and_the_task_layer_is_fresh_then_stale_then_gone() {
    let env = project();
    expect(&env.run(&["retain", RULE, "--kind", "directive"]), "kept n000001\n");
    handoff(&env, "t1", "scout", "load_config reads the settings file.");
    handoff(&env, "t1", "scout", "read_text returns the whole text of a file.");
    handoff(&env, "t1", "scout", "unused_helper does nothing and no code calls it.");
    handoff(&env, "t1", "scout", "The nightly job finishes before six o'clock.");
    handoff(&env, "t2", "scout", "Logs rotate at midnight.");

    let fresh = loadout(&env, &["--task", "t1", "loadout"]);
    assert!(fresh.contains("loadout: 5 notes, 5 fresh, 0 stale, 0 gone, about "), "{fresh}");
    assert!(!fresh.contains("check stale"), "no stale or gone note, no warning: {fresh}");

    // Change read_text and remove unused_helper.
    let core = env.read("app/core.py");
    env.write("app/core.py", &core.replace("return handle.read()", "return handle.read().strip()"));
    env.write("app/util.py", "def slugify(text):\n    \"\"\"Make a url friendly name.\"\"\"\n    return text.lower()\n");

    let text = loadout(&env, &["--task", "t1", "loadout"]);
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines[0], "== core");
    assert_eq!(lines[1], "- Always run the formatter before committing.  (<age> ago) #n000001");
    assert_eq!(lines[2], "== task t1");
    assert_eq!(
        &lines[3..7],
        [
            "- The nightly job finishes before six o'clock.  (<age> ago) [scout] #n000005",
            "- load_config reads the settings file.  (<age> ago) [scout] #n000002",
            "- read_text returns the whole text of a file.  (<age> ago, stale: code changed) [scout] #n000003",
            "- unused_helper does nothing and no code calls it.  (<age> ago, gone: code removed) [scout] #n000004",
        ]
    );
    assert!(lines[7].starts_with("loadout: 5 notes, 3 fresh, 1 stale, 1 gone, about "), "{}", lines[7]);
    assert_eq!(lines[8], "check stale and gone notes against the code before relying on them");
    assert_eq!(lines.len(), 9, "{text}");
    // The task t2 note is not part of the loadout of t1.
    assert!(!text.contains("Logs rotate"));

    // The figure in the closing line is what the printed lines cost.
    let spent: usize = lines[..7].iter().map(|l| cost_of_aged(l)).sum();
    assert!(lines[7].ends_with(&format!("about {spent} tokens")), "{} vs {spent}", lines[7]);
}

#[test]
fn a_directive_and_a_confirmed_note_make_the_core_and_the_core_layer_leaves_the_task_layer() {
    let env = project();
    expect(&env.run(&["retain", "The exporter writes csv files."]), "kept n000001\n");
    expect(&env.run(&["retain", "The exporter writes csv files."]), "reinforced n000001 (seen 2 times)\n");
    // Seen twice and by one role: not confirmed yet.
    expect(&env.run(&["loadout"]), "no notes\n");
    expect(&env.run(&["retain", "the exporter writes csv files"]), "reinforced n000001 (seen 3 times)\n");
    let by_repetition = loadout(&env, &["loadout"]);
    assert!(by_repetition.starts_with("== core\n- The exporter writes csv files.  (<age> ago, seen 3 times) #n000001\n"), "{by_repetition}");

    // A note two roles filed under one task is confirmed, and listed once, under core, with both roles.
    handoff(&env, "t1", "scout", "The scheduler retries failed jobs.");
    handoff(&env, "t1", "reviewer", "the scheduler retries failed jobs");
    handoff(&env, "t1", "scout", "Logs rotate at midnight.");
    let text = loadout(&env, &["--task", "t1", "loadout"]);
    let core = layer(&text, "== core");
    assert_eq!(ids(&core), ["n000002", "n000001"].map(String::from), "{text}");
    assert!(core[1].ends_with("[reviewer+scout] #n000002") && core[1].contains("seen 2 times"), "{}", core[1]);
    assert!(core[2].ends_with("(<age> ago, seen 3 times) #n000001"), "{}", core[2]);
    let task = layer(&text, "== task t1");
    assert_eq!(ids(&task), ["n000003"].map(String::from), "{text}");
    assert!(text.contains("loadout: 3 notes, 3 fresh, 0 stale, 0 gone, about "), "{text}");
}

#[test]
fn directives_come_before_confirmed_notes_and_both_are_newest_first() {
    let env = project();
    for _ in 0..3 {
        env.run(&["retain", "The exporter writes csv files."]);
    }
    expect(&env.run(&["retain", "First rule: keep functions short.", "--kind", "directive"]), "kept n000002\n");
    expect(&env.run(&["retain", "Second rule: name tests after behaviour.", "--kind", "directive"]), "kept n000003\n");
    let text = loadout(&env, &["loadout"]);
    assert_eq!(ids(&layer(&text, "== core")), ["n000003", "n000002", "n000001"].map(String::from), "{text}");
}

#[test]
fn a_small_budget_cuts_a_layer_and_counts_the_notes_left_out() {
    let env = project();
    for i in 0..8 {
        handoff(&env, "t1", "scout", &format!("Step {i} of the migration touches table number {i} only."));
    }
    let wide = loadout(&env, &["--task", "t1", "loadout", "--budget", "2000"]);
    let total = ids(&layer(&wide, "== task t1")).len();
    assert!(total >= 8, "{wide}");
    assert!(!wide.contains(" more"), "{wide}");
    let budget = 120;
    let text = loadout(&env, &["--task", "t1", "loadout", "--budget", &budget.to_string()]);
    let task = layer(&text, "== task t1");
    let shown = ids(&task).len();
    assert!(shown >= 1 && shown < total, "{text}");
    assert_eq!(task.last().unwrap(), &format!("  +{} more", total - shown), "{text}");
    let lines: Vec<&str> = text.lines().collect();
    let closing = lines.iter().position(|l| l.starts_with("loadout:")).unwrap();
    assert!(lines[closing].starts_with(&format!("loadout: {shown} notes, {shown} fresh, 0 stale, 0 gone, about ")), "{}", lines[closing]);
    let spent: usize = lines[..closing].iter().map(|l| cost_of_aged(l)).sum();
    assert!(spent <= budget, "{spent} tokens of {budget}: {text}");
    assert!(lines[closing].ends_with(&format!("about {spent} tokens")), "{}", lines[closing]);
}

#[test]
fn the_core_layer_takes_at_most_a_third_and_the_task_layer_the_rest() {
    let env = project();
    for i in 0..6 {
        expect(&env.run(&["retain", &format!("Rule {i}: never skip the review of change number {i}."), "--kind", "directive"]), &format!("kept n{:06}\n", i + 1));
    }
    handoff(&env, "t1", "scout", "The nightly job finishes before six o'clock.");
    let budget = 120;
    let text = loadout(&env, &["--task", "t1", "loadout", "--budget", &budget.to_string()]);
    let core = layer(&text, "== core");
    let shown = ids(&core).len();
    assert!(shown >= 1 && shown < 6, "{text}");
    assert_eq!(core.last().unwrap(), &format!("  +{} more", 6 - shown), "{text}");
    let core_cost: usize = core.iter().map(|l| cost(l)).sum();
    assert!(core_cost <= budget / 3, "core costs {core_cost} of {}: {text}", budget / 3);
    // The note of the task is printed with the budget the core layer left.
    assert_eq!(ids(&layer(&text, "== task t1")), ["n000007"].map(String::from), "{text}");
}

#[test]
fn the_related_layer_holds_at_most_four_notes_that_the_layers_above_did_not_print() {
    let env = project();
    expect(&env.run(&["retain", RULE, "--kind", "directive"]), "kept n000001\n");
    handoff(&env, "t1", "scout", "The slugify helper is used by the url builder.");
    for word in ["alpha", "beta", "gamma", "delta", "epsilon", "zeta"] {
        keep(&env, &format!("Slugify handles {word} inputs by lowercasing them first."));
    }
    let text = loadout(&env, &["--task", "t1", "loadout", "slugify"]);
    let related = layer(&text, "== related");
    assert!(!related.is_empty(), "{text}");
    let in_related = ids(&related);
    assert!(in_related.len() <= 4, "{text}");
    assert!(!in_related.contains(&"n000002".to_string()), "the note of the task is printed under its task: {text}");
    assert_eq!(ids(&layer(&text, "== task t1")), ["n000002"].map(String::from), "{text}");
    assert_eq!(ids(&layer(&text, "== core")), ["n000001"].map(String::from), "{text}");
    // The layers come in this order.
    let at = |title: &str| text.find(title).unwrap();
    assert!(at("== core") < at("== task t1") && at("== task t1") < at("== related"), "{text}");
    // Words that no note shares leave the layer out.
    assert!(!loadout(&env, &["--task", "t1", "loadout", "zzzqqq"]).contains("== related"));
    // The related layer is for words only.
    assert!(!loadout(&env, &["--task", "t1", "loadout"]).contains("== related"));
}

#[test]
fn the_related_layer_stays_within_a_third_of_the_budget() {
    let env = project();
    for i in 0..6 {
        keep(&env, &format!("Slugify handles input number {i} by lowercasing it first, then joining the words."));
    }
    let budget = 120;
    let text = loadout(&env, &["loadout", "slugify", "--budget", &budget.to_string()]);
    let related = layer(&text, "== related");
    assert!(!related.is_empty(), "{text}");
    assert!(related.iter().map(|l| cost(l)).sum::<usize>() <= budget / 3, "{text}");
    assert!(related.last().unwrap().ends_with(" more") || related.len() > 1, "{text}");
}

#[test]
fn a_loadout_never_changes_the_bank_and_gives_the_same_bytes_twice() {
    let env = project();
    expect(&env.run(&["retain", RULE, "--kind", "directive"]), "kept n000001\n");
    handoff(&env, "t1", "scout", "load_config reads the settings file.");
    handoff(&env, "t1", "reviewer", "read_text returns the whole text of a file.");
    handoff(&env, "t1", "reviewer", "Read_text returns the whole text of a file");
    let dir = bank_dir(&env);
    // Without its caches a bank that is opened the usual way writes them again; a loadout must not.
    let _ = fs::remove_file(dir.join("index.bin"));
    let _ = fs::remove_file(dir.join("vectors.bin"));
    let before = folder_state(&dir);
    assert!(before.contains_key("notes.jsonl"));
    for args in [
        vec!["loadout"],
        vec!["--task", "t1", "loadout"],
        vec!["--task", "t1", "loadout", "settings", "file"],
        vec!["loadout", "formatter", "--budget", "30"],
        vec!["--task", "none", "loadout", "--budget", "1"],
    ] {
        let first = env.run(&args);
        let second = env.run(&args);
        assert_eq!(first.code, 0, "{args:?}: {}", first.err);
        assert_eq!(no_age(&first.out), no_age(&second.out), "{args:?}");
        assert_eq!(differing(&before, &folder_state(&dir)), Vec::<String>::new(), "{args:?} changed these files of the bank");
    }
    // Making the index current is not a change to the bank, even when the code moved.
    env.write("app/core.py", &env.read("app/core.py").replace("return handle.read()", "return handle.read().strip()"));
    assert!(loadout(&env, &["--task", "t1", "loadout"]).contains("stale: code changed"));
    assert_eq!(differing(&before, &folder_state(&dir)), Vec::<String>::new());
}

#[test]
fn the_task_comes_from_the_variable_and_the_flag_wins() {
    let env = project();
    handoff(&env, "t1", "scout", "The exporter writes csv files.");
    handoff(&env, "t2", "scout", "The scheduler retries failed jobs.");
    let by_variable = env.run_with(&["loadout"], &[("PGR_TASK", "t1")]);
    assert!(by_variable.out.contains("== task t1\n") && by_variable.out.contains("exporter"), "{}", by_variable.out);
    let flag_wins = env.run_with(&["--task", "t2", "loadout"], &[("PGR_TASK", "t1")]);
    assert!(flag_wins.out.contains("== task t2\n") && flag_wins.out.contains("scheduler") && !flag_wins.out.contains("exporter"), "{}", flag_wins.out);
}

// ---- pack ----

/// The lines of a section of a pack answer (a line that starts with `== ` opens the next one).
fn section<'a>(text: &'a str, title: &str) -> Vec<&'a str> {
    layer(text, &format!("== {title}"))
}

/// What `pgr about NAME` prints.
fn about(env: &Env, name: &str) -> String {
    let run = env.run(&["about", name]);
    assert_eq!(run.code, 0, "{}", run.err);
    run.out
}

#[test]
fn names_in_the_task_get_the_block_of_pgr_about_each_once_in_order() {
    let env = project();
    let task = "change `load_config` so that read_text(path) is called once, then Store.reload and read_text again";
    let run = env.run(&["pack", task]);
    assert_eq!(run.code, 0, "{}", run.err);
    let expected = format!("== names\n{}{}{}", about(&env, "load_config"), about(&env, "read_text"), about(&env, "Store.reload"));
    assert!(run.out.starts_with(&expected), "{}", run.out);
    // The hits for the definitions that have a block are left out of the find section.
    let plain = env.run(&["find", "-k", "8", task]).out;
    assert!(plain.contains("load_config app/core.py:4-6") || plain.contains("read_text app/core.py:9-12"), "the search finds them: {plain}");
    let find = section(&run.out, "find");
    for named in ["load_config app/core.py:4-6", "read_text app/core.py:9-12", "Store.reload app/core.py:"] {
        assert!(find.iter().all(|l| !l.starts_with(named)), "{named} is under names already: {}", run.out);
    }
}

#[test]
fn every_shape_of_code_name_is_found() {
    let env = project();
    env.write("app/shapes.py", "def fooBar():\n    \"\"\"Camel case.\"\"\"\n    return 1\n\n\nclass WidgetMaker:\n    \"\"\"Pascal case.\"\"\"\n\n    def build(self):\n        return 2\n");
    for (task, name) in [
        ("fix read_text soon", "read_text"),
        ("check Store.get now", "Store.get"),
        ("see app/util.py::slugify here", "app/util.py::slugify"),
        ("the fooBar helper", "fooBar"),
        ("the WidgetMaker class", "WidgetMaker"),
        ("make slugify(text) faster", "slugify"),
        ("use `report` here", "report"),
    ] {
        let run = env.run(&["pack", task]);
        assert_eq!(run.code, 0, "{task}: {}", run.err);
        let expected = format!("== names\n{}", about(&env, name));
        assert!(run.out.starts_with(&expected), "{task}: {}", run.out);
    }
    // Plain words, a capital at the start only, and a word that is only followed by a space are not names.
    for task in ["change how Store reads the settings", "make slugify (text) faster", "report on the main loop"] {
        let run = env.run(&["pack", task]);
        assert!(!run.out.contains("== names"), "{task}: {}", run.out);
    }
}

#[test]
fn a_name_with_two_definitions_is_skipped_and_at_most_four_names_get_a_block() {
    let env = project();
    env.write("app/twin_a.py", "def twin_task():\n    return 1\n");
    env.write("app/twin_b.py", "def twin_task():\n    return 2\n");
    let run = env.run(&["pack", "fix `twin_task` and read_text"]);
    let names = section(&run.out, "names");
    assert_eq!(names.join("\n") + "\n", format!("== names\n{}", about(&env, "read_text")), "{}", run.out);

    let five = "`read_text` `parse_text` `load_config` `unused_helper` `slugify`";
    let run = env.run(&["pack", five, "--budget", "4000"]);
    let expected = format!("== names\n{}{}{}{}", about(&env, "read_text"), about(&env, "parse_text"), about(&env, "load_config"), about(&env, "unused_helper"));
    assert!(run.out.starts_with(&expected), "{}", run.out);
    assert!(!section(&run.out, "names").iter().any(|l| l.starts_with("slugify ")), "the fifth name gets no block: {}", run.out);
}

#[test]
fn names_are_cut_to_half_the_budget_at_a_line_end_with_a_count_of_the_lines_left() {
    let env = project();
    // A definition that many functions call, so that its block is far longer than half the budget.
    let callers: String = (0..14).map(|i| format!("\n\ndef user_{i}():\n    return hub_call()\n")).collect();
    env.write("app/hub.py", &format!("def hub_call():\n    return 1\n{callers}"));
    assert_eq!(env.run(&["refresh"]).code, 0);
    let full = about(&env, "hub_call");
    let total = full.lines().count();
    assert!(total >= 16 && (full.len() + 3) / 4 > 120, "{full}");
    let budget = 120;
    let run = env.run(&["pack", "look at `hub_call`", "--budget", &budget.to_string()]);
    assert_eq!(run.code, 0, "{}", run.err);
    let names = section(&run.out, "names");
    let (more, kept) = names[1..].split_last().unwrap();
    assert_eq!(*kept, full.lines().take(kept.len()).collect::<Vec<_>>(), "{}", run.out);
    assert_eq!(*more, format!("  +{} more lines", total - kept.len()), "{}", run.out);
    assert!(names[1..].iter().map(|l| cost(l)).sum::<usize>() <= budget / 2, "{}", run.out);
    // With room for it all, nothing is cut.
    let wide = env.run(&["pack", "look at `hub_call`", "--budget", "2000"]);
    assert!(wide.out.starts_with(&format!("== names\n{full}")) && !wide.out.contains("more lines"), "{}", wide.out);
}

#[test]
fn the_budget_is_shared_names_up_to_half_and_the_rest_in_turn_with_the_unused_share_passed_on() {
    let env = project();
    let task = "change how `load_config` reads the settings file and what read_text returns";
    for budget in [120usize, 300, 800, 2000] {
        let run = env.run(&["pack", task, "--budget", &budget.to_string()]);
        assert_eq!(run.code, 0, "{}", run.err);
        let mut spent = 0usize;
        let mut names = 0usize;
        let mut title = String::new();
        for line in run.out.lines() {
            if line.starts_with("== ") {
                title = line[3..].to_string();
            } else {
                spent += cost(line);
                if title == "names" {
                    names += cost(line);
                }
            }
        }
        assert!(names <= budget / 2, "names cost {names} at budget {budget}: {}", run.out);
        // A line that is cut costs one token more than its share, at most once per section.
        assert!(spent <= budget + 3, "{spent} tokens at budget {budget}: {}", run.out);
    }
    // Only the graph engine is on: names and graph. Nothing else is waiting, so the graph section
    // gets everything the names do not use and not a third of it.
    let run = env.run(&["--engines", "graph", "pack", "change how load_config reads the settings file", "--budget", "120"]);
    assert!(run.out.starts_with("== names\n") && !run.out.contains("== find") && !run.out.contains("== memory"), "{}", run.out);
    let names: usize = section(&run.out, "names")[1..].iter().map(|l| cost(l)).sum();
    let graph = section(&run.out, "graph");
    let spent: usize = graph[1..].iter().map(|l| cost(l)).sum();
    assert!(spent > 120 / 3 && spent <= 120 - names + 1, "the graph section spent {spent} tokens after {names} for the names: {}", run.out);
}

#[test]
fn engines_choose_the_sections_and_an_empty_answer_says_so() {
    let env = project();
    let task = "change `load_config` and how it reads the settings file";
    let graph_only = env.run(&["--engines", "graph", "pack", task]).out;
    assert!(graph_only.starts_with("== names\n") && graph_only.contains("== graph\n") && !graph_only.contains("== find"), "{graph_only}");
    let search_only = env.run(&["--engines", "search", "pack", task]).out;
    assert!(search_only.starts_with("== find\n") && !search_only.contains("== names") && !search_only.contains("== graph"), "{search_only}");
    // Without the graph the names are not looked up, so the hit stays in the find section.
    assert!(search_only.contains("load_config app/core.py:4-6"), "{search_only}");
    expect(&env.run(&["--engines", "memory", "pack", "zzzqqq"]), "nothing found for this task\n");
    expect(&env.run(&["--engines", "graph", "pack", "zzzqqq"]), "nothing found for this task\n");
}

#[test]
fn pack_gives_the_same_bytes_twice() {
    let env = project();
    handoff(&env, "t1", "scout", "load_config reads the settings file.");
    for args in [
        vec!["pack", "change `load_config` and read_text(path)"],
        vec!["--task", "t1", "pack", "change `load_config` and read_text(path)"],
        vec!["pack", "change how the settings file is read", "--budget", "100"],
    ] {
        let first = env.run(&args);
        let second = env.run(&args);
        assert_eq!(first.code, 0, "{args:?}: {}", first.err);
        assert_eq!(no_age(&first.out), no_age(&second.out), "{args:?}");
    }
}

#[test]
fn with_a_task_the_memory_section_holds_the_core_and_task_layers_without_the_closing_lines() {
    let env = project();
    expect(&env.run(&["retain", RULE, "--kind", "directive"]), "kept n000001\n");
    handoff(&env, "t1", "scout", "load_config reads the settings file.");
    handoff(&env, "t1", "scout", "read_text returns the whole text of a file.");
    handoff(&env, "t2", "scout", "Logs rotate at midnight.");
    env.write("app/core.py", &env.read("app/core.py").replace("return handle.read()", "return handle.read().strip()"));
    let task = "change how the settings file is read";
    let run = env.run(&["--task", "t1", "pack", task]);
    assert_eq!(run.code, 0, "{}", run.err);
    let text = no_age(&run.out);
    let at = text.find("== memory\n").unwrap_or_else(|| panic!("{text}"));
    let memory = &text[at..];
    assert!(memory.starts_with("== memory\n== core\n- Always run the formatter before committing."), "{memory}");
    let task_layer = layer(memory, "== task t1");
    assert_eq!(ids(&task_layer), ["n000002", "n000003"].map(String::from), "{memory}");
    assert!(task_layer[2].contains("stale: code changed") || task_layer[1].contains("stale: code changed"), "{memory}");
    assert!(!text.contains("loadout:") && !text.contains("check stale"), "no closing lines: {text}");
    assert!(!text.contains("Logs rotate"), "the notes of another task are not included: {text}");
    // The sections before memory keep their order.
    assert!(text.find("== find").unwrap() < at, "{text}");
    // The memory section is the layers and not the notes the task text recalls.
    let without = env.run(&["pack", task]);
    assert!(!without.out.contains("== core") && !without.out.contains("== task"), "{}", without.out);
    // The variable names the task too.
    let by_variable = env.run_with(&["pack", task], &[("PGR_TASK", "t1")]);
    assert_eq!(no_age(&by_variable.out), text);
}

// ---- MCP ----

fn call(id: u64, name: &str, arguments: Value) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "method": "tools/call", "params": {"name": name, "arguments": arguments}})
}

/// Sends the requests, one per line, closes standard input and returns the answers.
fn talk(env: &Env, args: &[&str], requests: &[Value]) -> Vec<Value> {
    let input: String = requests.iter().map(|r| format!("{r}\n")).collect();
    let mut full = args.to_vec();
    full.push("mcp");
    let run = env.run_stdin(&full, &input);
    assert_eq!(run.code, 0, "{}", run.err);
    run.out.lines().map(|l| serde_json::from_str(l).unwrap_or_else(|e| panic!("not JSON ({e}): {l}"))).collect()
}

fn text_of(answer: &Value) -> &str {
    answer["result"]["content"][0]["text"].as_str().unwrap_or_else(|| panic!("no text in {answer}"))
}

#[test]
fn the_server_lists_the_new_tools_and_each_has_a_one_sentence_description() {
    let env = project();
    let answers = talk(&env, &[], &[json!({"jsonrpc": "2.0", "id": 1, "method": "tools/list"})]);
    let tools = answers[0]["result"]["tools"].as_array().unwrap();
    let names: Vec<&str> = tools.iter().map(|t| t["name"].as_str().unwrap()).collect();
    assert_eq!(names, ["ask", "find", "recall", "retain", "pack", "handoff", "loadout"]);
    let by_name = |name: &str| tools.iter().find(|t| t["name"] == name).unwrap();
    for name in ["pack", "handoff", "loadout"] {
        let description = by_name(name)["description"].as_str().unwrap();
        assert!(description.ends_with('.') && !description.contains(". "), "one sentence: {description}");
    }
    assert_eq!(by_name("handoff")["inputSchema"]["required"], json!(["task", "text"]));
    assert!(by_name("handoff")["inputSchema"]["properties"]["from"].is_object());
    assert_eq!(by_name("loadout")["inputSchema"]["properties"]["budget"]["type"], "integer");
    assert!(by_name("pack")["inputSchema"]["properties"]["for_task"].is_object());
}

#[test]
fn the_tools_give_the_text_of_the_commands() {
    let env = project();
    let answers = talk(
        &env,
        &[],
        &[
            call(1, "handoff", json!({"task": "t1", "text": "load_config reads the settings file.", "from": "scout"})),
            call(2, "handoff", json!({"task": "t1", "text": "The exporter writes csv files.", "kind": "experience", "tags": ["ops"]})),
            call(3, "loadout", json!({"task": "t1", "query": "exporter"})),
            call(4, "pack", json!({"task": "change `load_config`", "for_task": "t1", "budget": 400})),
        ],
    );
    assert_eq!(answers.len(), 4);
    assert_eq!(text_of(&answers[0]), "kept n000001\nhandoff t1: 1 notes from scout, 1 name code\n");
    assert_eq!(text_of(&answers[1]), "kept n000002\nhandoff t1: 1 notes from agent, 0 name code\n");
    for answer in &answers {
        assert_eq!(answer["result"]["isError"], false, "{answer}");
    }
    let tags = tags_of(&notes_in(&env)["n000002"]);
    assert!(tags.contains(&"ops".to_string()) && tags.contains(&"task:t1".to_string()) && tags.contains(&"from:agent".to_string()), "{tags:?}");

    // The same calls on the command line.
    let line = |args: &[&str]| no_age(&env.run(args).out);
    assert_eq!(no_age(text_of(&answers[2])), line(&["--task", "t1", "loadout", "exporter"]));
    assert!(text_of(&answers[2]).starts_with("== task t1\n"), "{}", text_of(&answers[2]));
    assert_eq!(no_age(text_of(&answers[3])), line(&["--task", "t1", "pack", "change `load_config`", "--budget", "400"]));
    assert!(text_of(&answers[3]).starts_with("== names\n"), "{}", text_of(&answers[3]));
    assert!(text_of(&answers[3]).contains("== task t1\n"), "{}", text_of(&answers[3]));
}

#[test]
fn the_server_uses_the_task_and_role_it_was_started_with_when_a_call_gives_none() {
    let env = project();
    let answers = talk(
        &env,
        &["--task", "t7", "--from", "judge"],
        &[call(1, "handoff", json!({"task": "t1", "text": "The exporter writes csv files."})), call(2, "loadout", json!({}))],
    );
    assert_eq!(text_of(&answers[0]), "kept n000001\nhandoff t1: 1 notes from judge, 0 name code\n");
    // No task in the call: the server's own task t7, which has no notes yet.
    assert_eq!(text_of(&answers[1]), "no notes\n");
}

#[test]
fn a_bad_name_in_a_tool_call_is_a_tool_error_and_the_server_goes_on() {
    let env = project();
    let answers = talk(
        &env,
        &[],
        &[
            call(1, "handoff", json!({"task": "..\\x", "text": "a fact"})),
            call(2, "handoff", json!({"task": "t1", "text": "a fact", "from": "a/b"})),
            call(3, "handoff", json!({"task": "", "text": "a fact"})),
            call(4, "loadout", json!({"task": "bad name"})),
            call(5, "pack", json!({"task": "anything", "for_task": "x\\y"})),
            call(6, "handoff", json!({"task": "t1", "text": "The exporter writes csv files."})),
        ],
    );
    assert_eq!(answers.len(), 6);
    let errors = [
        "pgr: ..\\x is not a valid task name\n",
        "pgr: a/b is not a valid role name\n",
        "pgr:  is not a valid task name\n",
        "pgr: bad name is not a valid task name\n",
        "pgr: x\\y is not a valid task name\n",
    ];
    for (answer, expected) in answers.iter().zip(errors) {
        assert_eq!(answer["result"]["isError"], true, "{answer}");
        assert_eq!(text_of(answer), expected);
    }
    assert_eq!(answers[5]["result"]["isError"], false);
    assert_eq!(text_of(&answers[5]), "kept n000001\nhandoff t1: 1 notes from agent, 0 name code\n");
    assert_eq!(notes_in(&env).len(), 1, "nothing was kept for a bad name");
    // A missing required argument is an invalid-parameters error of the protocol.
    let missing = talk(&env, &[], &[call(1, "handoff", json!({"text": "no task"}))]);
    assert_eq!(missing[0]["error"]["code"], -32602);
}

// ---- help and version ----

#[test]
fn the_version_is_0_2_0() {
    let env = Env::new();
    for flag in ["--version", "-V"] {
        let run = env.run_in(&env.proj, &[flag]);
        expect(&run, "pgr 0.2.0\n");
    }
}

#[test]
fn the_help_lists_the_new_commands_and_the_new_flags() {
    let env = Env::new();
    let help = env.run_in(&env.proj, &["help"]);
    assert_eq!(help.code, 0, "{}", help.err);
    assert_eq!(env.run_in(&env.proj, &["--help"]).out, help.out);
    assert_eq!(env.run_in(&env.proj, &["-h"]).out, help.out);
    let lines: Vec<&str> = help.out.lines().collect();
    let usage = lines.iter().find(|l| l.starts_with("usage:")).unwrap();
    for flag in ["--root DIR", "--engines LIST", "--bank NAME", "--task NAME", "--from ROLE"] {
        assert!(usage.contains(flag), "{usage}");
    }
    let memory = lines.iter().position(|l| *l == "memory:").unwrap();
    let combined = lines.iter().position(|l| *l == "combined:").unwrap();
    let under_memory = &lines[memory..combined];
    assert!(under_memory.iter().any(|l| l.trim_start().starts_with("handoff TEXT...")), "{}", help.out);
    assert!(under_memory.iter().any(|l| l.trim_start().starts_with("loadout [WORDS...] [--budget N]")), "{}", help.out);
    for variable in ["PGR_BANK", "PGR_TASK", "PGR_ROLE"] {
        assert!(help.out.contains(variable), "{variable}");
    }
    assert!(help.out.contains("--bank NAME") && help.out.contains("--task NAME") && help.out.contains("--from ROLE"));
    assert!(help.out.contains("pack TASK [--budget N]"));
    // No command at all still prints the help on standard error and exits with 2.
    let none = env.run_in(&env.proj, &[]);
    assert_eq!(none.code, 2);
    assert!(none.err.contains("handoff TEXT..."));
}

// ---- the review of the first build ----

/// The ids of the notes `pgr recall WORDS` finds, in the order it prints them.
fn recalled(env: &Env, vars: &[(&str, &str)], words: &str) -> Vec<String> {
    let run = env.run_with(&["recall", words, "-k", "50", "--max-tokens", "100000"], vars);
    assert_eq!(run.code, 0, "{}", run.err);
    ids(&run.out.lines().collect::<Vec<_>>())
}

/// The `T` of the closing line `loadout: ..., about T tokens`.
fn tokens_of(text: &str) -> usize {
    let line = text.lines().find(|l| l.starts_with("loadout: ")).unwrap_or_else(|| panic!("no closing line in {text}"));
    line.rsplit("about ").next().unwrap().trim_end_matches(" tokens").parse().unwrap_or_else(|_| panic!("{line}"))
}

#[test]
fn the_related_layer_is_the_recall_for_the_words_also_for_notes_that_share_none_of_them() {
    let env = project();
    keep(&env, "load_config keeps the parsed settings in a module cache.");
    keep(&env, "load_config is called by main and by Store.reload.");
    keep(&env, "The exporter writes csv files.");
    // The second note is linked to the first by the name they share; it has no word of the question.
    let found = recalled(&env, &[], "settings");
    assert_eq!(found, ["n000001", "n000002"].map(String::from), "recall links the second note to the first");
    let text = loadout(&env, &["loadout", "settings"]);
    assert_eq!(ids(&layer(&text, "== related")), found, "{text}");
}

#[test]
fn with_an_embedding_model_the_related_layer_keeps_the_notes_recall_finds_by_meaning() {
    let Some(model) = lxr_text::Embedder::locate() else {
        eprintln!("no embedding model on this machine: test skipped");
        return;
    };
    let model = model.to_string_lossy().into_owned();
    let vars = [("LXR_MODEL_DIR", model.as_str())];
    let env = Env::with_project();
    assert_eq!(env.run_with(&["refresh"], &vars).code, 0);
    for fact in [
        "load_config reads the settings file.",
        "The exporter writes csv files.",
        "Logs rotate at midnight.",
        "The scheduler retries failed jobs.",
        "Release notes go into the changelog.",
    ] {
        let run = env.run_with(&["retain", fact], &vars);
        assert_eq!(run.code, 0, "{}", run.err);
    }
    // What a model finds is up to the model: only the first question is known to find a note, and
    // for every question the layer must hold what `recall` finds, whatever that is.
    assert!(!recalled(&env, &vars, "configuration").is_empty(), "the model finds the settings note for a question that shares no word with it");
    for words in ["configuration", "nightly data dump", "zzzqqq"] {
        let found = recalled(&env, &vars, words);
        let run = env.run_with(&["loadout", words], &vars);
        assert_eq!(run.code, 0, "{}", run.err);
        let expected: Vec<String> = found.into_iter().take(4).collect();
        assert_eq!(ids(&layer(&no_age(&run.out), "== related")), expected, "{words:?}: {}", run.out);
    }
}

#[test]
fn a_digest_the_bank_builds_from_notes_about_one_definition_is_not_in_the_core_or_the_task_layer() {
    let env = project();
    for fact in [
        "load_config reads the configuration file at the path it is given.",
        "load_config hands the text on to parse_text and returns the settings.",
        "load_config is called by main and by Store.reload.",
        "load_config raises no error when the file is empty.",
    ] {
        handoff(&env, "t3", "scout", fact);
    }
    let stats = env.run(&["memory", "stats"]).out;
    assert!(stats.contains("observation 1"), "the bank summarised the four notes: {stats}");
    // One role, nothing repeated: nothing is confirmed, and the digest counts as nothing.
    expect(&env.run(&["--task", "unrelated-task", "loadout"]), "no notes\n");
    let text = loadout(&env, &["--task", "t3", "loadout"]);
    assert!(layer(&text, "== core").is_empty(), "{text}");
    assert_eq!(ids(&layer(&text, "== task t3")).len(), 4, "only the four notes themselves: {text}");
    assert!(!text.contains("seen "), "{text}");
    assert!(text.contains("loadout: 4 notes, 4 fresh, 0 stale, 0 gone, about "), "{text}");
}

#[test]
fn a_note_too_big_for_its_layer_is_counted_in_the_more_line_and_does_not_hide_the_older_notes() {
    let env = project();
    for fact in ["The exporter writes csv files.", "The scheduler retries failed jobs.", "Logs rotate at midnight."] {
        handoff(&env, "t1", "scout", fact);
    }
    let long = (0..420).map(|i| format!("step{i}")).collect::<Vec<_>>().join(" ");
    let run = env.run_stdin(&["--task", "t1", "--from", "scout", "handoff", "--whole", "-"], &long);
    assert_eq!(run.code, 0, "{}", run.err);
    let text = loadout(&env, &["--task", "t1", "loadout"]);
    let task = layer(&text, "== task t1");
    assert_eq!(ids(&task), ["n000003", "n000002", "n000001"].map(String::from), "{text}");
    assert_eq!(task.last().unwrap(), &"  +1 more", "{text}");
    assert!(text.contains("loadout: 3 notes, 3 fresh, 0 stale, 0 gone, about "), "{text}");

    // The same in the core layer: two short directives and a newer one that is far too long.
    let env = project();
    expect(&env.run(&["retain", RULE, "--kind", "directive"]), "kept n000001\n");
    expect(&env.run(&["retain", "Second rule: name tests after behaviour.", "--kind", "directive"]), "kept n000002\n");
    expect(&env.run(&["retain", &long, "--kind", "directive", "--whole"]), "kept n000003\n");
    let text = loadout(&env, &["loadout"]);
    let core = layer(&text, "== core");
    assert_eq!(ids(&core), ["n000002", "n000001"].map(String::from), "{text}");
    assert_eq!(core.last().unwrap(), &"  +1 more", "{text}");
}

#[test]
fn a_task_layer_with_no_core_above_it_gets_the_whole_budget() {
    let env = project();
    handoff(&env, "t", "scout", "The exporter writes csv files.");
    let wide = loadout(&env, &["--task", "t", "loadout", "--budget", "1000"]);
    let needed = tokens_of(&wide);
    assert!(wide.contains("== task t\n- The exporter"), "{wide}");
    // The title of a core layer that prints nothing costs nothing: the exact cost is enough.
    assert_eq!(loadout(&env, &["--task", "t", "loadout", "--budget", &needed.to_string()]), wide);
}

#[test]
fn a_fact_repeated_inside_one_handoff_is_one_note_in_the_closing_line() {
    let env = project();
    let run = env.run_stdin(&["--task", "t1", "--from", "scout", "handoff", "-"], &"retrying the connection to the cache server\n".repeat(3));
    // The fact written three times in one call is one sighting: the note is seen once, and no
    // line says that it was reinforced.
    expect(&run, "kept n000001\nhandoff t1: 1 notes from scout, 0 name code\n");
    assert_eq!(notes_in(&env)["n000001"]["proof_count"], 1);
    expect(&env.run(&["--task", "other-task", "loadout"]), "no notes\n");
    let run = env.run_stdin(&["--task", "t1", "--from", "scout", "handoff", "-"], &"load_config reads the settings file.\n".repeat(2));
    let closing = run.out.lines().last().unwrap().to_string();
    assert_eq!(closing, "handoff t1: 1 notes from scout, 1 name code", "{}", run.out);
    // A second call is a second sighting.
    let again = env.run_stdin(&["--task", "t1", "--from", "scout", "handoff", "-"], "retrying the connection to the cache server\n");
    expect(&again, "reinforced n000001 (seen 2 times)\nhandoff t1: 1 notes from scout, 0 name code\n");
}

#[test]
fn a_fact_repeated_inside_one_retain_call_is_one_sighting() {
    let env = project();
    expect(&env.run(&["retain", "The queue holds one hundred jobs.\nthe queue holds one hundred jobs\nThe queue holds one hundred jobs."]), "kept n000001\n");
    assert_eq!(notes_in(&env)["n000001"]["proof_count"], 1);
    // A note that was there before is reinforced once by the call, however often the call repeats it.
    expect(&env.run(&["retain", "The queue holds one hundred jobs.\nThe queue holds one hundred jobs."]), "reinforced n000001 (seen 2 times)\n");
    assert_eq!(notes_in(&env)["n000001"]["proof_count"], 2);
}

#[test]
fn a_loadout_never_costs_more_than_its_budget_and_a_small_budget_is_raised_to_120() {
    let env = project();
    for i in 0..5 {
        let rule = format!("Rule {i}: never skip the review of change number {i}.");
        expect(&env.run(&["retain", &rule, "--kind", "directive"]), &format!("kept n{:06}\n", i + 1));
    }
    for i in 0..8 {
        handoff(&env, "t1", "scout", &format!("Step {i} of the migration touches table number {i} only."));
    }
    for i in 0..4 {
        keep(&env, &format!("Cache number {i} is emptied by the nightly job."));
    }
    for budget in (120..=240usize).step_by(3) {
        let run = env.run(&["--task", "t1", "loadout", "cache", "--budget", &budget.to_string()]);
        assert_eq!(run.code, 0, "{}", run.err);
        let tokens = tokens_of(&run.out);
        assert!(tokens <= budget, "budget {budget} printed {tokens} tokens: {}", run.out);
    }
    // A budget under 120 is raised to it, so that no budget is left with room for nothing.
    let small = env.run(&["--task", "t1", "loadout", "cache", "--budget", "1"]);
    let floor = env.run(&["--task", "t1", "loadout", "cache", "--budget", "120"]);
    assert_eq!((no_age(&small.out), small.err.as_str()), (no_age(&floor.out), "pgr: budget raised to 120\n"));
}

#[test]
fn a_bad_role_in_the_environment_is_refused_by_loadout_and_pack_as_it_is_by_handoff() {
    let env = project();
    for command in [vec!["loadout"], vec!["--task", "t", "loadout", "cache"], vec!["pack", "load_config"], vec!["--task", "t", "pack", "load_config"]] {
        let run = env.run_with(&command, &[("PGR_ROLE", "bad role")]);
        assert_eq!(run.code, 2, "{command:?}: {}", run.out);
        assert_eq!(run.out, "", "{command:?}");
        assert_eq!(run.err.trim_end(), "pgr: bad role is not a valid role name", "{command:?}");
    }
    // The flag wins over the variable, so a bad variable is not looked at then.
    assert_eq!(env.run_with(&["--from", "scout", "loadout"], &[("PGR_ROLE", "bad role")]).code, 0);
}

#[test]
fn a_bank_name_must_also_be_a_folder_name_of_its_own_on_every_system() {
    let env = project();
    for value in ["x.", "x..", "...", "....", "NUL", "nul", "Con", "aux.txt", "COM1", "lpt9", "prn"] {
        let run = env.run(&["--bank", value, "status"]);
        assert_eq!(run.code, 2, "{value:?}: {}", run.err);
        assert_eq!(run.err.trim_end(), format!("pgr: {value} is not a valid bank name"), "{value:?}");
        let by_variable = env.run_with(&["status"], &[("PGR_BANK", value)]);
        assert_eq!(by_variable.code, 2, "{value:?}");
    }
    for value in ["console", "com10", "nulls", "x.y", ".x", "lpt", "a-b_c"] {
        let run = env.run(&["--bank", value, "memory", "path"]);
        assert_eq!(run.code, 0, "{value:?}: {}", run.err);
    }
    // A task or a role is a tag and not a folder, so the rule of the contract is all it has.
    assert_eq!(env.run(&["--task", "x.", "--from", "nul", "loadout"]).code, 0);
}

// ---- the second review ----

/// A second repository beside the project, with a function the project does not have.
fn second_repository(env: &Env) -> PathBuf {
    let other = env.base.join("proj2");
    fs::create_dir_all(&other).unwrap();
    fs::write(other.join("tool.py"), "def run_tool():\n    \"\"\"Run it.\"\"\"\n    return 1\n").unwrap();
    other
}

/// Files a handoff to the bank `team` from the repository at `root`.
fn file_from(env: &Env, root: &PathBuf, text: &str) {
    let run = env.run_at(root, &["--bank", "team", "--task", "t1", "--from", "scout", "handoff", text], &[]);
    assert_eq!(run.code, 0, "{}", run.err);
}

/// What `loadout --task t1` of the bank `team` prints in the repository at `root`, with the ages replaced.
fn team_loadout(env: &Env, root: &PathBuf) -> String {
    let run = env.run_at(root, &["--bank", "team", "--task", "t1", "loadout"], &[]);
    assert_eq!(run.code, 0, "{}", run.err);
    no_age(&run.out)
}

/// Rewrites the log of a bank without the repository of any anchor, as a bank of an older version had
/// it, and drops the cache files so that the log is read again.
fn strip_repository(dir: &PathBuf) {
    let log = dir.join("notes.jsonl");
    let lines: Vec<String> = fs::read_to_string(&log)
        .unwrap()
        .lines()
        .map(|line| {
            let mut note: Value = serde_json::from_str(line).unwrap();
            if let Some(anchors) = note.get_mut("anchors").and_then(Value::as_array_mut) {
                for anchor in anchors {
                    anchor.as_object_mut().unwrap().remove("repo");
                }
            }
            note.to_string()
        })
        .collect();
    fs::write(&log, lines.join("\n") + "\n").unwrap();
    for cache in ["index.bin", "vectors.bin"] {
        let _ = fs::remove_file(dir.join(cache));
    }
}

#[test]
fn a_note_about_code_of_another_repository_is_not_judged_and_sorts_between_fresh_and_stale() {
    let env = project();
    let other = second_repository(&env);
    file_from(&env, &env.proj, "load_config hands the text of the file to parse_text.");
    file_from(&env, &other, "run_tool returns one for every caller.");
    file_from(&env, &other, "The nightly job finishes before six o'clock.");
    // Every anchor carries the repository the note was made in, and the two repositories differ.
    let dir = env.home.join("banks").join("team");
    let repos: Vec<String> = fs::read_to_string(dir.join("notes.jsonl"))
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .filter(|note| note["id"] == "n000001" || note["id"] == "n000002")
        .map(|note| note["anchors"][0]["repo"].as_str().unwrap_or("").to_string())
        .collect();
    assert!(repos.len() == 2 && !repos[0].is_empty() && !repos[1].is_empty() && repos[0] != repos[1], "{repos:?}");

    // The second repository changes the function its own note is about.
    fs::write(other.join("tool.py"), "def run_tool():\n    \"\"\"Run it.\"\"\"\n    return 2\n").unwrap();
    let seen = team_loadout(&env, &other);
    let task = layer(&seen, "== task t1");
    assert_eq!(ids(&task), ["n000003", "n000001", "n000002"].map(String::from), "{seen}");
    assert!(task[2].ends_with("(<age> ago, other repository) [scout] #n000001"), "{seen}");
    assert!(task[3].ends_with("(<age> ago, stale: code changed) [scout] #n000002"), "{seen}");
    assert!(seen.contains("loadout: 3 notes, 1 fresh, 1 stale, 0 gone, 1 unchecked, about "), "{seen}");
    assert!(seen.contains("check stale and gone notes"), "{seen}");

    // The project reads the note of the other repository as not judged, not as gone, and has nothing to check.
    let seen = team_loadout(&env, &env.proj);
    assert_eq!(ids(&layer(&seen, "== task t1")), ["n000003", "n000001", "n000002"].map(String::from), "{seen}");
    assert!(seen.contains("(<age> ago, other repository) [scout] #n000002"), "{seen}");
    assert!(seen.contains("loadout: 3 notes, 2 fresh, 0 stale, 0 gone, 1 unchecked, about ") && !seen.contains("check stale"), "{seen}");
    for command in ["recall", "reflect"] {
        let run = env.run(&["--bank", "team", command, "run_tool"]);
        assert_eq!(run.code, 0, "{}", run.err);
        assert!(run.out.contains(", other repository)"), "{command}: {}", run.out);
        assert!(!run.out.contains("gone: code removed"), "{command}: {}", run.out);
    }

    // A note from before the bank recorded the repository counts as made in the repository that reads it.
    strip_repository(&dir);
    let seen = team_loadout(&env, &env.proj);
    assert!(seen.contains("gone: code removed") && !seen.contains("other repository"), "{seen}");
    assert!(seen.contains("loadout: 3 notes, 2 fresh, 0 stale, 1 gone, about "), "{seen}");
}

/// A linked worktree of the project, made as git makes one: its `.git` is a file that points into
/// the `.git` folder of the project, whose `worktrees/wt` folder names that folder as the common one.
fn linked_worktree(env: &Env) -> PathBuf {
    let admin = env.proj.join(".git").join("worktrees").join("wt");
    fs::create_dir_all(&admin).unwrap();
    fs::write(admin.join("commondir"), "../..\n").unwrap();
    let wt = env.base.join("wt");
    for rel in ["app/core.py", "app/cli.py", "app/util.py"] {
        fs::create_dir_all(wt.join(rel).parent().unwrap()).unwrap();
        fs::copy(env.proj.join(rel), wt.join(rel)).unwrap();
    }
    fs::write(wt.join(".git"), format!("gitdir: {}\n", admin.display())).unwrap();
    wt
}

#[test]
fn a_worktree_of_the_project_is_the_same_repository_for_the_notes() {
    let env = project();
    let wt = linked_worktree(&env);
    file_from(&env, &env.proj, "load_config hands the text of the file to parse_text.");
    // The worktree changes the function the note is about. It is the repository the note was made in,
    // so the note is judged there, and the same holds the other way round.
    let core = fs::read_to_string(wt.join("app/core.py")).unwrap();
    fs::write(wt.join("app/core.py"), core.replace("def load_config(path):", "def load_config(path, strict=False):")).unwrap();
    let seen = team_loadout(&env, &wt);
    assert!(seen.contains("(<age> ago, stale: code changed) [scout] #n000001"), "{seen}");
    assert!(!seen.contains("other repository") && seen.contains("check stale and gone notes"), "{seen}");
    // A note made in the worktree is stamped like one made in the project, and the project judges it.
    file_from(&env, &wt, "parse_text returns the lines it was given in a list.");
    let seen = team_loadout(&env, &env.proj);
    assert!(!seen.contains("other repository"), "{seen}");
    let repos: Vec<String> = fs::read_to_string(env.home.join("banks").join("team").join("notes.jsonl"))
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .filter_map(|note| note["anchors"][0]["repo"].as_str().map(String::from))
        .collect();
    assert!(repos.len() >= 2 && repos.iter().all(|r| r == &repos[0]), "{repos:?}");
    // A clone is another repository: it has a `.git` folder of its own.
    let clone = env.base.join("clone");
    fs::create_dir_all(clone.join(".git")).unwrap();
    fs::create_dir_all(clone.join("app")).unwrap();
    fs::copy(env.proj.join("app/core.py"), clone.join("app/core.py")).unwrap();
    let seen = team_loadout(&env, &clone);
    assert!(seen.contains(", other repository) [scout] #n000001"), "{seen}");
}

#[test]
fn pack_and_loadout_print_a_digest_and_a_long_signature_cut_after_a_whole_word() {
    let env = project();
    let args: Vec<String> = (1..=14).map(|n| format!("argument_number_{n}")).collect();
    env.write("app/long.py", &format!("def a_function_with_a_long_name({}):\n    return 1\n", args.join(", ")));
    // The signature is cut to 240 characters by the index; the cut goes back to the end of an argument.
    let run = env.run(&["pack", "refactor a_function_with_a_long_name"]);
    assert_eq!(run.code, 0, "{}", run.err);
    let names = layer(&run.out, "== names");
    assert!(names[1].ends_with(",..."), "{}", run.out);
    // A digest of the bank is cut to 600 characters, after a whole word too.
    for n in 0..3 {
        let details: Vec<String> = (1..=30).map(|i| format!("detail{i}")).collect();
        handoff(&env, "t1", "scout", &format!("parse_text case{n} {}", details.join(" ")));
    }
    for args in [vec!["pack", "parse_text detail11"], vec!["loadout", "parse_text detail11", "--budget", "900"]] {
        let run = env.run(&args);
        assert_eq!(run.code, 0, "{}", run.err);
        let seen = no_age(&run.out);
        let digest = seen.lines().find(|l| l.starts_with("- parse_text: ")).unwrap_or_else(|| panic!("no digest: {seen}"));
        let text = digest.split("  (<age>").next().unwrap();
        let last = text.split_whitespace().last().unwrap();
        assert!(last.starts_with("detail") && last[6..].chars().all(|c| c.is_ascii_digit()), "{args:?}: {text}");
    }
}

#[test]
fn a_budget_under_120_tokens_is_raised_to_120_and_said_once() {
    let env = project();
    handoff(&env, "t1", "scout", "The exporter writes csv files.");
    let wide = env.run(&["--task", "t1", "loadout", "--budget", "120"]);
    assert!(!wide.err.contains("budget raised"), "{}", wide.err);
    let pack = env.run(&["pack", "load_config", "--budget", "120"]);
    assert!(!pack.err.contains("budget raised"), "{}", pack.err);
    for budget in ["1", "50", "119"] {
        let run = env.run(&["--task", "t1", "loadout", "--budget", budget]);
        assert_eq!((no_age(&run.out), run.err.as_str()), (no_age(&wide.out), "pgr: budget raised to 120\n"), "loadout {budget}");
        let run = env.run(&["pack", "load_config", "--budget", budget]);
        assert_eq!((no_age(&run.out), run.err.as_str()), (no_age(&pack.out), "pgr: budget raised to 120\n"), "pack {budget}");
    }
    let tools = talk(&env, &[], &[call(1, "loadout", json!({"task": "t1", "budget": 5})), call(2, "pack", json!({"task": "load_config", "budget": 5}))]);
    assert!(tools.iter().all(|answer| answer.get("error").is_none() && answer["result"]["isError"] != true), "{tools:?}");
}

#[test]
fn a_cut_layer_ends_at_the_first_note_that_does_not_fit_so_the_order_of_priority_holds() {
    let env = project();
    for i in 0..10 {
        let words = (0..22).map(|k| format!("word{i}x{k}")).collect::<Vec<_>>().join(" ");
        let text = if i % 2 == 0 { format!("Short note {i}.") } else { format!("Long note {i}: {words}") };
        handoff(&env, "t1", "scout", &text);
    }
    let wide = loadout(&env, &["--task", "t1", "loadout", "--budget", "5000"]);
    let order = ids(&layer(&wide, "== task t1"));
    assert_eq!(order.len(), 10, "{wide}");
    let mut cut = 0;
    for budget in (150..=700).step_by(10) {
        let text = loadout(&env, &["--task", "t1", "loadout", "--budget", &budget.to_string()]);
        let task = layer(&text, "== task t1");
        let shown = ids(&task);
        assert_eq!(shown, order[..shown.len()].to_vec(), "budget {budget}: {text}");
        if shown.len() < order.len() {
            assert_eq!(task.last().unwrap(), &format!("  +{} more", order.len() - shown.len()), "budget {budget}: {text}");
            cut += 1;
        }
    }
    assert!(cut > 3, "the budgets cut the layer {cut} times");
}

#[test]
fn a_digest_the_bank_built_is_not_in_the_related_layer_when_the_task_layer_is_printed() {
    let env = project();
    for fact in [
        "load_config reads the configuration file at the path it is given.",
        "load_config hands the text on to parse_text and returns the settings.",
        "load_config is called by main and by Store.reload.",
        "load_config raises no error when the file is empty.",
    ] {
        handoff(&env, "t3", "scout", fact);
    }
    assert!(env.run(&["memory", "stats"]).out.contains("observation 1"));
    let text = loadout(&env, &["--task", "t3", "loadout", "load_config"]);
    assert_eq!(ids(&layer(&text, "== task t3")).len(), 4, "{text}");
    assert!(layer(&text, "== related").is_empty(), "the facts are printed once, not again as a digest: {text}");
    assert!(text.contains("loadout: 4 notes, 4 fresh, 0 stale, 0 gone, about "), "{text}");
}

/// Sets every time in the log of the bank of the project back by `seconds`, and drops the cache files.
fn age_notes(env: &Env, seconds: i64) {
    let dir = bank_dir(env);
    let log = dir.join("notes.jsonl");
    let lines: Vec<String> = fs::read_to_string(&log)
        .unwrap()
        .lines()
        .map(|line| {
            let mut note: Value = serde_json::from_str(line).unwrap();
            for key in ["created_at", "updated_at", "mentioned_at"] {
                if let Some(time) = note[key].as_i64() {
                    note[key] = json!(time - seconds);
                }
            }
            note.to_string()
        })
        .collect();
    fs::write(&log, lines.join("\n") + "\n").unwrap();
    for cache in ["index.bin", "vectors.bin"] {
        let _ = fs::remove_file(dir.join(cache));
    }
}

#[test]
fn a_line_is_costed_as_it_is_printed_so_the_figure_is_true_at_every_age() {
    let env = project();
    for i in 0..40 {
        handoff(&env, "t1", "agent", &format!("Step {i} of the migration touches table number {i} only and nothing else before it stops."));
    }
    // The notes are set back by this much more at each step: now, 45 days, 400 days.
    for (seconds, age) in [(0, "s ago"), (45 * 86_400, "45d ago"), (355 * 86_400, "400d ago")] {
        if seconds > 0 {
            age_notes(&env, seconds);
        }
        for budget in [120, 300, 600] {
            let run = env.run(&["--task", "t1", "loadout", "--budget", &budget.to_string()]);
            assert_eq!(run.code, 0, "{}", run.err);
            assert!(run.out.contains(age), "{}", run.out);
            let lines: Vec<&str> = run.out.lines().collect();
            let closing = lines.iter().position(|l| l.starts_with("loadout:")).unwrap();
            let spent: usize = lines[..closing].iter().map(|l| cost(l)).sum();
            assert_eq!(spent, tokens_of(&run.out), "the figure is what the printed lines cost ({age}, {budget}): {}", run.out);
            assert!(spent <= budget, "{spent} tokens of {budget} ({age}): {}", run.out);
        }
    }
}

#[test]
fn two_names_with_a_slash_between_them_get_a_block_each() {
    let env = project();
    let run = env.run(&["pack", "refactor load_config/parse_text to share a reader"]);
    assert_eq!(run.code, 0, "{}", run.err);
    let expected = format!("== names\n{}{}", about(&env, "load_config"), about(&env, "parse_text"));
    assert!(run.out.starts_with(&expected), "{}", run.out);
    // A path with a definition after it is still found as one name.
    let run = env.run(&["pack", "look at app/core.py::load_config and app/util.py::slugify"]);
    let names = section(&run.out, "names").join("\n");
    assert!(names.contains("load_config app/core.py:4-6") && names.contains("slugify app/util.py:"), "{}", run.out);
}
