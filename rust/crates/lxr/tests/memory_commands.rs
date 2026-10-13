//! The memory commands, the code awareness of notes, and the `== memory` section of
//! `ask` and `pack`.

mod common;
use common::{expect, Env};

const NOTE: &str = "load_config reads the settings file and must never be called with a relative path.";
const RULE: &str = "Always run the formatter before committing.";

/// A project with an index and a bank holding two notes.
fn with_notes() -> Env {
    let env = Env::with_project();
    assert_eq!(env.run(&["refresh"]).code, 0);
    expect(&env.run(&["retain", NOTE, "--tag", "cfg"]), "kept n000001\n");
    expect(&env.run(&["retain", RULE, "--kind", "directive"]), "kept n000002\n");
    env
}

#[test]
fn retain_then_recall_then_forget() {
    let env = Env::with_project();
    expect(&env.run(&["recall", "anything"]), "no notes\n");
    assert!(!env.index_dir().join("memory").exists(), "recalling must not create the bank");

    expect(&env.run(&["retain", NOTE, "--tag", "cfg"]), "kept n000001\n");
    let again = env.run(&["retain", NOTE]);
    expect(&again, "reinforced n000001 (seen 2 times)\n");

    let recall = env.run(&["recall", "load_config", "settings"]);
    assert_eq!(recall.code, 0, "{}", recall.err);
    let line = recall.out.lines().next().unwrap();
    assert!(line.starts_with(&format!("- {NOTE}  (")) && line.ends_with(", seen 2 times) #n000001"), "{line}");
    assert_eq!(recall.out.lines().count(), 1);

    expect(&env.run(&["memory", "forget", "n000001"]), "forgot n000001\n");
    expect(&env.run(&["memory", "forget", "n000001"]), "no note n000001\n");
    expect(&env.run(&["recall", "load_config", "settings"]), "no notes\n");
}

#[test]
fn a_directive_comes_first_and_the_filters_apply() {
    let env = with_notes();
    let recall = env.run(&["recall", "load_config", "settings", "formatter"]);
    let lines: Vec<&str> = recall.out.lines().collect();
    assert_eq!(lines.len(), 2, "{}", recall.out);
    assert!(lines[0].starts_with(&format!("- {RULE}  (")) && lines[0].ends_with(") #n000002"), "{}", lines[0]);
    assert!(lines[1].contains("load_config reads the settings file") && lines[1].ends_with(" #n000001"), "{}", lines[1]);

    let tagged = env.run(&["recall", "load_config", "--tag", "cfg"]);
    assert!(tagged.out.contains("#n000001") && !tagged.out.contains("#n000002"), "{}", tagged.out);
    expect(&env.run(&["recall", "load_config", "--tag", "nothing"]), "no notes\n");
    expect(&env.run(&["recall", "load_config", "--since", "2999-01-01"]), "no notes\n");
    assert!(env.run(&["recall", "load_config", "--since", "2000-01-01"]).out.contains("#n000001"));
    expect(&env.run(&["recall", "load_config", "--until", "2000-01-01"]), "no notes\n");
    let limited = env.run(&["recall", "load_config", "settings", "formatter", "-k", "1"]);
    assert_eq!(limited.out.lines().count(), 1, "{}", limited.out);
    let tiny = env.run(&["recall", "load_config", "--max-tokens", "1", "--budget", "low"]);
    assert_eq!(tiny.code, 0, "{}", tiny.err);
}

#[test]
fn a_note_goes_stale_when_the_code_it_names_changes_and_gone_when_it_is_removed() {
    let env = with_notes();
    let fresh = env.run(&["recall", "load_config"]);
    assert!(!fresh.out.contains("stale") && !fresh.out.contains("gone"), "{}", fresh.out);

    let core = env.read("app/core.py");
    env.write("app/core.py", &core.replace("return parse_text(read_text(path))", "text = read_text(path)\n    return parse_text(text)"));
    let stale = env.run(&["recall", "load_config"]);
    assert!(stale.out.contains(", stale: code changed) #n000001"), "{}", stale.out);
    assert!(stale.err.starts_with("pgr: index refreshed, 1 files changed"), "{}", stale.err);

    env.write("app/core.py", &core.replace("def load_config(path):", "def load_config_v2(path):"));
    let gone = env.run(&["recall", "load_config"]);
    assert!(gone.out.contains(", gone: code removed) #n000001"), "{}", gone.out);

    env.write("app/core.py", &core);
    let back = env.run(&["recall", "load_config"]);
    assert!(!back.out.contains("stale") && !back.out.contains("gone"), "{}", back.out);
}

#[test]
fn ask_adds_a_memory_section_only_when_a_note_qualifies() {
    let env = with_notes();
    // Nothing in the bank is about slugify.
    expect(&env.run(&["ask", "def:slugify"]), "== def:slugify\nslugify app/util.py:1-3 (text)\n");

    let run = env.run(&["ask", "def:load_config"]);
    assert_eq!(run.code, 0, "{}", run.err);
    let (answer, memory) = run.out.split_once("== memory\n").expect("a memory section");
    assert_eq!(answer, "== def:load_config\nload_config app/core.py:4-6 (path)\n");
    assert!(memory.contains("load_config reads the settings file") && memory.contains("#n000001"), "{memory}");
    assert!(!memory.contains("formatter"), "{memory}");
    assert!(memory.lines().count() <= 4);

    // Asking for the notes directly leaves out the closing section.
    let direct = env.run(&["ask", "def:load_config", "recall:settings"]);
    assert_eq!(direct.out.matches("== memory").count(), 0, "{}", direct.out);
    assert!(direct.out.contains("== recall:settings\n"), "{}", direct.out);
    assert!(direct.out.contains("- load_config reads the settings file"), "{}", direct.out);

    // The engine switched off: no section.
    let off = env.run(&["--engines", "graph,search", "ask", "def:load_config"]);
    assert_eq!(off.out, "== def:load_config\nload_config app/core.py:4-6 (path)\n");
}

#[test]
fn ask_without_a_bank_has_no_memory_section_and_creates_no_folder() {
    let env = Env::with_project();
    expect(&env.run(&["ask", "def:slugify"]), "== def:slugify\nslugify app/util.py:1-3 (text)\n");
    assert!(!env.index_dir().join("memory").exists());
}

#[test]
fn reflect_sorts_notes_into_sections() {
    let env = with_notes();
    let run = env.run(&["reflect", "load_config", "formatter"]);
    assert_eq!(run.code, 0, "{}", run.err);
    let text = run.out;
    let directives = text.find("directives:\n").expect("directives section");
    let notes = text.find("notes:\n").expect("notes section");
    assert!(directives < notes, "{text}");
    assert!(text.contains("by subject:\n"), "{text}");
    assert!(!text.contains("observations:"), "observations are empty here: {text}");
    // A directive is a standing rule, so the bank offers it for any words. Without one, nothing matches.
    let unrelated = env.run(&["reflect", "qqqzzzxxx"]);
    assert!(unrelated.out.starts_with("directives:\n") && !unrelated.out.contains("notes:\n"), "{}", unrelated.out);
    expect(&env.run(&["memory", "forget", "n000002"]), "forgot n000002\n");
    expect(&env.run(&["reflect", "qqqzzzxxx"]), "no notes\n");
}

#[test]
fn memory_list_stats_and_path() {
    let env = with_notes();
    let list = env.run(&["memory", "list"]);
    assert_eq!(list.code, 0, "{}", list.err);
    let lines: Vec<&str> = list.out.lines().collect();
    assert_eq!(lines.len(), 2, "{}", list.out);
    assert!(lines[0].contains("#n000002") && lines[0].contains("directive"), "newest first: {}", lines[0]);
    assert!(lines[1].contains("#n000001") && lines[1].contains("world"), "{}", lines[1]);
    assert_eq!(env.run(&["memory", "list", "-k", "1"]).out.lines().count(), 1);

    let stats = env.run(&["memory", "stats"]);
    assert!(stats.out.starts_with("2 notes (world 1, experience 0, observation 0, directive 1)"), "{}", stats.out);

    let path = env.run(&["memory", "path"]);
    assert_eq!(path.out.trim_end(), env.index_dir().join("memory").to_string_lossy());

    let status = env.run(&["status"]);
    assert!(status.out.ends_with("| memory 2 notes.\n"), "{}", status.out);
}

#[test]
fn memory_commands_do_not_read_the_code_index() {
    let env = Env::with_project();
    expect(&env.run(&["memory", "list"]), "no notes\n");
    expect(&env.run(&["memory", "stats"]), "0 notes (world 0, experience 0, observation 0, directive 0), 0 entities, 0 links, 0 bytes\n");
    expect(&env.run(&["memory", "forget", "n000009"]), "no note n000009\n");
    assert!(fs_is_empty(&env.home), "no index may be built by these commands");
}

fn fs_is_empty(dir: &std::path::Path) -> bool {
    std::fs::read_dir(dir).unwrap().next().is_none()
}

#[test]
fn retain_options_whole_context_when_and_kind() {
    let env = Env::with_project();
    let text = "The cache is cleared at midnight. The cache is rebuilt at dawn.";
    let split = env.run(&["retain", text]);
    assert!(split.out.lines().count() >= 1, "{}", split.out);
    let whole = env.run(&["retain", "--whole", "--kind", "experience", "--context", "standup", "--when", "2026-01-02", "Fixed the nightly job; it was a typo."]);
    assert_eq!(whole.out.lines().count(), 1, "{}", whole.out);
    assert!(whole.out.starts_with("kept n"), "{}", whole.out);
    let list = env.run(&["memory", "list", "-k", "1"]);
    assert!(list.out.contains("experience"), "{}", list.out);
}

#[test]
fn a_named_bank_is_shared_by_every_repository() {
    let env = Env::with_project();
    let other = env.base.join("other");
    std::fs::create_dir_all(&other).unwrap();
    std::fs::write(other.join("m.py"), "def lonely():\n    return 1\n").unwrap();
    let vars = [("PGR_BANK", "shared")];

    let kept = env.run_with(&["retain", "The shared bank remembers lonely things."], &vars);
    assert_eq!(kept.out, "kept n000001\n", "{}", kept.err);
    let recalled = env.run_at(&other, &["recall", "shared", "bank"], &vars);
    assert!(recalled.out.contains("The shared bank remembers lonely things."), "{}", recalled.out);
    assert!(env.home.join("banks").join("shared").is_dir());
    // Without the variable the other repository has its own, empty bank.
    expect(&env.run_at(&other, &["recall", "shared", "bank"], &[]), "no notes\n");
}

#[test]
fn memory_usage_errors() {
    let env = Env::with_project();
    for args in [
        vec!["memory"],
        vec!["memory", "bogus"],
        vec!["memory", "forget"],
        vec!["retain"],
        vec!["retain", "x", "--kind", "observation"],
        vec!["retain", "x", "--when", "yesterday"],
        vec!["recall"],
        vec!["recall", "x", "--since", "soon"],
        vec!["reflect"],
    ] {
        let run = env.run(&args);
        assert_eq!(run.code, 2, "{args:?}: {}", run.err);
        assert_eq!(run.out, "");
    }
}

#[test]
fn pack_gives_sections_within_a_third_of_the_budget() {
    let env = Env::with_project();
    let task = "change how load_config reads the settings file";
    let bare = env.run(&["pack", task]);
    assert_eq!(bare.code, 0, "{}", bare.err);
    assert!(bare.out.starts_with("== find\nload_config app/core.py:4-6 (path)\n"), "{}", bare.out);
    assert!(bare.out.contains("== graph\nquery: "), "{}", bare.out);
    assert!(!bare.out.contains("== memory"), "no notes yet: {}", bare.out);

    expect(&env.run(&["retain", NOTE]), "kept n000001\n");
    let with_memory = env.run(&["pack", task]);
    assert!(with_memory.out.contains("== memory\n- load_config reads the settings file"), "{}", with_memory.out);

    for budget in [90usize, 300, 800] {
        let run = env.run(&["pack", task, "--budget", &budget.to_string()]);
        // A section holds whole groups of lines within its share; only a first group that is
        // too long alone is cut to the share's size in characters (one token more at most).
        let share = (budget / 3).max(1);
        let mut sections = 0usize;
        let mut spent = 0usize;
        for line in run.out.lines().chain(std::iter::once("== end")) {
            if line.starts_with("== ") {
                assert!(spent <= share + 2, "section of {spent} tokens at budget {budget}: {}", run.out);
                sections += 1;
                spent = 0;
            } else {
                spent += (line.len() + 3) / 4 + 1;
            }
        }
        assert!(sections >= 3, "find, graph, memory and the sentinel: {}", run.out);
    }
    expect(&env.run(&["--engines", "memory", "pack", "zzzqqq"]), "none\n");
}

#[test]
fn with_the_graph_engine_off_notes_need_no_index_and_are_not_judged_against_code() {
    let env = Env::with_project();
    let memory_only = ["--engines", "memory"];
    let retain = env.run(&[memory_only[0], memory_only[1], "retain", NOTE]);
    expect(&retain, "kept n000001\n");
    assert_eq!(retain.err, "");
    let recall = env.run(&[memory_only[0], memory_only[1], "recall", "load_config", "settings"]);
    assert_eq!(recall.code, 0, "{}", recall.err);
    assert!(recall.out.contains("load_config reads the settings file") && !recall.out.contains("stale"), "{}", recall.out);
    assert!(!env.index_dir().join("manifest.bin").exists(), "no index may be built for a memory-only run");
}
