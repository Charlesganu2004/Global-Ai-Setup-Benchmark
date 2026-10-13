//! The index lifecycle: building, the status line, the automatic refresh after an edit
//! or a deletion, PGR_NO_REFRESH, damaged files, the lock and the folders pgr refuses.

mod common;
use common::{expect, Env, Locked};
use std::fs;
use std::time::{Duration, SystemTime};

#[test]
fn status_before_and_after_refresh() {
    let env = Env::with_project();
    expect(&env.run(&["status"]), "pgr: proj | no index yet (run: pgr refresh)\n");
    // Status never builds.
    assert!(fs::read_dir(&env.home).unwrap().next().is_none());

    let refresh = env.run(&["refresh"]);
    assert_eq!(refresh.code, 0, "{}", refresh.err);
    assert!(
        refresh.out.starts_with("pgr: proj | graph ready (12 symbols, 21 links, ")
            && refresh.out.ends_with(" old) | search ready (14 chunks, words only) | memory 0 notes.\n"),
        "{}",
        refresh.out
    );
    assert!(refresh.err.starts_with("pgr: index built in ") && refresh.err.contains(" ms\n"), "{}", refresh.err);
    assert!(refresh.err.ends_with("pgr: no embedding model found, search uses words only\n"), "{}", refresh.err);

    let status = env.run(&["status"]);
    assert!(
        status.out.starts_with("pgr: proj | graph ready (12 symbols, 21 links, ")
            && status.out.ends_with(" old) | search ready (14 chunks, words only) | memory 0 notes.\n"),
        "{}",
        status.out
    );
    assert_eq!(status.err, "");
    assert!(env.index_dir().join("manifest.bin").is_file());
    assert!(env.index_dir().join("graph.bin").is_file());
    assert!(env.index_dir().join("facts.bin").is_file());
    assert!(env.index_dir().join("search").is_dir());
}

#[test]
fn a_second_refresh_with_nothing_changed_does_not_rebuild() {
    let env = Env::with_project();
    env.run(&["refresh"]);
    let built = fs::metadata(env.index_dir().join("graph.bin")).unwrap().modified().unwrap();
    std::thread::sleep(Duration::from_millis(60));
    let again = env.run(&["refresh"]);
    assert_eq!(again.code, 0);
    assert!(!again.err.contains("index built") && !again.err.contains("index refreshed"), "{}", again.err);
    let after = fs::metadata(env.index_dir().join("graph.bin")).unwrap().modified().unwrap();
    assert_eq!(built, after, "graph.bin must not be written when nothing changed");
}

#[test]
fn a_query_builds_a_missing_index_and_says_so_on_standard_error() {
    let env = Env::with_project();
    let first = env.run(&["def", "slugify"]);
    expect(&first, "slugify app/util.py:1-3 (text)\n");
    assert!(first.err.starts_with("pgr: index built in ") && first.err.ends_with(" ms\n"), "{}", first.err);
    let second = env.run(&["def", "slugify"]);
    expect(&second, "slugify app/util.py:1-3 (text)\n");
    assert_eq!(second.err, "");
}

#[test]
fn an_edited_file_is_picked_up_before_the_answer() {
    let env = Env::with_project();
    expect(&env.run(&["callers", "slugify"]), "0 callers of slugify\n");
    let mut util = env.read("app/util.py");
    util.push_str("\n\ndef clean_name(text):\n    return slugify(text)\n");
    env.write("app/util.py", &util);

    let run = env.run(&["callers", "slugify"]);
    expect(&run, "1 caller of slugify:\nclean_name app/util.py:11-12 @12\n");
    assert!(run.err.starts_with("pgr: index refreshed, 1 files changed, ") && run.err.ends_with(" ms\n"), "{}", run.err);
    // The search index follows too.
    expect(&env.run(&["find", "-k", "1", "clean_name"]), "clean_name app/util.py:11-12 (text)\n");
}

#[test]
fn a_new_file_is_picked_up_and_a_deleted_file_disappears() {
    let env = Env::with_project();
    expect(&env.run(&["def", "extra_one"]), "extra_one: not found\n");
    env.write("app/extra.py", "def extra_one():\n    return 1\n");
    let added = env.run(&["def", "extra_one"]);
    expect(&added, "extra_one app/extra.py:1-2 ()\n");
    assert!(added.err.starts_with("pgr: index refreshed, 1 files changed, "), "{}", added.err);

    env.remove("app/util.py");
    let removed = env.run(&["def", "slugify", "extra_one"]);
    expect(&removed, "slugify: not found\nextra_one app/extra.py:1-2 ()\n");
    assert!(removed.err.starts_with("pgr: index refreshed, 1 files changed, "), "{}", removed.err);
    assert!(env.run(&["find", "slugify"]).out.lines().all(|l| !l.contains("app/util.py")));
    let status = env.run(&["status"]);
    assert!(status.out.contains("graph ready (11 symbols"), "{}", status.out);
}

#[test]
fn a_file_with_a_new_time_and_the_same_bytes_is_not_rebuilt() {
    let env = Env::with_project();
    env.run(&["refresh"]);
    let path = env.proj.join("app/util.py");
    let file = fs::OpenOptions::new().write(true).open(&path).unwrap();
    file.set_modified(SystemTime::now() + Duration::from_secs(5)).unwrap();
    drop(file);
    let run = env.run(&["def", "slugify"]);
    expect(&run, "slugify app/util.py:1-3 (text)\n");
    assert_eq!(run.err, "", "same bytes, so nothing to rebuild");
}

#[test]
fn pgr_no_refresh_answers_from_the_index_as_it_is() {
    let env = Env::with_project();
    let none = env.run_with(&["def", "slugify"], &[("PGR_NO_REFRESH", "1")]);
    assert_eq!((none.code, none.out.as_str(), none.err.as_str()), (1, "", "pgr: no index yet (run: pgr refresh)\n"));

    env.run(&["refresh"]);
    let mut util = env.read("app/util.py");
    util.push_str("\n\ndef clean_name(text):\n    return slugify(text)\n");
    env.write("app/util.py", &util);

    let stale = env.run_with(&["callers", "slugify"], &[("PGR_NO_REFRESH", "1")]);
    expect(&stale, "0 callers of slugify\n");
    assert_eq!(stale.err, "");
    expect(&env.run(&["callers", "slugify"]), "1 caller of slugify:\nclean_name app/util.py:11-12 @12\n");
}

#[test]
fn a_damaged_graph_is_rebuilt_by_a_normal_query_and_reported_by_no_refresh() {
    let env = Env::with_project();
    env.run(&["refresh"]);
    let graph = env.index_dir().join("graph.bin");
    fs::write(&graph, b"this is not a graph").unwrap();

    let blocked = env.run_with(&["def", "slugify"], &[("PGR_NO_REFRESH", "1")]);
    assert_eq!(blocked.code, 1, "{}", blocked.err);
    assert!(blocked.err.starts_with("pgr: ") && blocked.err.contains("cannot be read"), "{}", blocked.err);

    let healed = env.run(&["def", "slugify"]);
    expect(&healed, "slugify app/util.py:1-3 (text)\n");
    assert!(healed.err.starts_with("pgr: index built in "), "{}", healed.err);
    assert_eq!(env.run(&["def", "slugify"]).err, "");
}

#[test]
fn a_damaged_manifest_or_missing_search_folder_means_a_rebuild() {
    let env = Env::with_project();
    env.run(&["refresh"]);
    fs::write(env.index_dir().join("manifest.bin"), b"junk").unwrap();
    let run = env.run(&["def", "slugify"]);
    expect(&run, "slugify app/util.py:1-3 (text)\n");
    assert!(run.err.starts_with("pgr: index built in "), "{}", run.err);

    fs::remove_dir_all(env.index_dir().join("search")).unwrap();
    let run = env.run(&["find", "-k", "1", "slugify"]);
    assert_eq!(run.code, 0, "{}", run.err);
    assert!(run.out.starts_with("slugify app/util.py:1-3"), "{}", run.out);
    assert!(env.index_dir().join("search").is_dir());
}

#[test]
fn refresh_force_rebuilds_everything() {
    let env = Env::with_project();
    env.run(&["refresh"]);
    let forced = env.run(&["refresh", "--force"]);
    assert_eq!(forced.code, 0);
    assert!(forced.err.starts_with("pgr: index built in "), "{}", forced.err);
}

#[test]
fn a_stale_lock_file_is_ignored() {
    let env = Env::with_project();
    env.run(&["refresh"]);
    env.write("app/extra.py", "def extra_one():\n    return 1\n");
    let lock = env.index_dir().join("refresh.lock");
    fs::write(&lock, "999999\n").unwrap();
    let file = fs::OpenOptions::new().write(true).open(&lock).unwrap();
    file.set_modified(SystemTime::now() - Duration::from_secs(700)).unwrap();
    drop(file);
    let run = env.run(&["def", "extra_one"]);
    expect(&run, "extra_one app/extra.py:1-2 ()\n");
    assert!(!lock.exists(), "the lock must be released");
}

#[test]
fn a_folder_with_a_skip_file_is_left_alone() {
    let env = Env::with_project();
    env.write(".local-index-skip", "");
    for args in [vec!["def", "main"], vec!["status"], vec!["refresh"], vec!["ask", "def:main"]] {
        let run = env.run(&args);
        assert_eq!(run.code, 0, "{args:?}: {}", run.err);
        assert_eq!(run.out, "pgr: off for proj (it holds a .local-index-skip file)\n", "{args:?}");
    }
    assert!(fs::read_dir(&env.home).unwrap().next().is_none(), "nothing may be indexed");
}

#[test]
fn the_home_folder_and_the_folders_above_it_are_left_alone() {
    let env = Env::with_project();
    let home = env.proj.join("deep").join("home");
    fs::create_dir_all(&home).unwrap();
    let home_text = home.to_string_lossy().into_owned();
    let run = env.run_with(&["def", "main"], &[("USERPROFILE", &home_text), ("HOME", &home_text)]);
    assert_eq!(run.code, 0, "{}", run.err);
    assert_eq!(run.out, "pgr: off for proj (the home folder or a folder above it)\n");
    // The home folder itself.
    let run = env.run_at(&home, &["status"], &[("USERPROFILE", &home_text), ("HOME", &home_text)]);
    assert_eq!(run.out, "pgr: off for home (the home folder or a folder above it)\n");
}

#[test]
fn a_repository_with_no_source_files_still_has_an_index() {
    let env = Env::new();
    env.write("notes.txt", "nothing to index here\n");
    let run = env.run(&["refresh"]);
    assert_eq!(run.code, 0, "{}", run.err);
    assert!(run.out.contains("graph ready (0 symbols, 0 links, "), "{}", run.out);
    expect(&env.run(&["def", "anything"]), "anything: not found\n");
    expect(&env.run(&["find", "anything"]), "none\n");
}

#[test]
fn non_utf8_source_does_not_stop_the_build() {
    let env = Env::with_project();
    fs::write(env.proj.join("app/latin.py"), b"def caf\xe9_name():\n    return b'\xff\xfe'\n").unwrap();
    let run = env.run(&["def", "slugify"]);
    expect(&run, "slugify app/util.py:1-3 (text)\n");
}

#[test]
fn a_repository_with_more_than_twenty_thousand_source_files_is_left_alone() {
    let env = Env::new();
    for folder in 0..21 {
        let dir = env.proj.join(format!("pkg{folder}"));
        fs::create_dir_all(&dir).unwrap();
        let count = if folder == 20 { 1 } else { 1000 };
        for file in 0..count {
            fs::write(dir.join(format!("m{file}.py")), "x = 1\n").unwrap();
        }
    }
    let run = env.run(&["def", "x"]);
    assert_eq!(run.code, 0, "{}", run.err);
    assert_eq!(run.out, "pgr: off for proj (more than 20000 source files)\n");
    assert!(fs::read_dir(&env.home).unwrap().next().is_none(), "nothing may be indexed");
}

#[test]
fn a_folder_that_cannot_be_read_stops_the_refresh_instead_of_dropping_its_files() {
    let env = Env::with_project();
    env.write("app/private/vault.py", "def open_vault():\n    return 1\n");
    let built = env.run(&["refresh"]);
    assert_eq!(built.code, 0, "{}", built.err);
    expect(&env.run(&["def", "open_vault"]), "open_vault app/private/vault.py:1-2 ()\n");

    let Some(locked) = Locked::new(&env.proj.join("app/private")) else { return };
    // `refresh` and every query that makes the index current stop, name the folder, and print no answer.
    let refresh = env.run(&["refresh"]);
    assert_eq!(refresh.code, 1, "stdout was: {}", refresh.out);
    assert_eq!(refresh.out, "");
    assert!(refresh.err.starts_with("pgr: cannot read the folder app/private in proj: "), "{}", refresh.err);
    assert!(refresh.err.contains("the index was not changed"), "{}", refresh.err);
    let query = env.run(&["def", "open_vault"]);
    assert_eq!(query.code, 1, "stdout was: {}", query.out);
    assert_eq!(query.out, "");
    assert!(query.err.contains("cannot read the folder app/private"), "{}", query.err);
    // The index still holds the file: it was not mistaken for a deleted one.
    let kept = env.run_with(&["def", "open_vault"], &[("PGR_NO_REFRESH", "1")]);
    expect(&kept, "open_vault app/private/vault.py:1-2 ()\n");
    let counts = built.out.split(" old)").next().unwrap().rsplit_once(", ").unwrap().0.to_string();
    let status = env.run(&["status"]);
    assert!(status.out.starts_with(&counts), "the index changed: {} after {counts}", status.out);

    // Once the folder can be read again, the next run finds nothing changed.
    drop(locked);
    let again = env.run(&["refresh"]);
    assert_eq!(again.code, 0, "{}", again.err);
    assert!(!again.err.contains("index built") && !again.err.contains("index refreshed"), "{}", again.err);
    expect(&env.run(&["def", "open_vault"]), "open_vault app/private/vault.py:1-2 ()\n");
}

#[test]
fn a_folder_that_cannot_be_read_stops_the_first_build_too() {
    let env = Env::with_project();
    env.write("app/private/vault.py", "def open_vault():\n    return 1\n");
    let Some(_locked) = Locked::new(&env.proj.join("app/private")) else { return };
    let run = env.run(&["refresh"]);
    assert_eq!(run.code, 1, "stdout was: {}", run.out);
    assert!(run.err.contains("cannot read the folder app/private"), "{}", run.err);
    // Nothing was built from the part of the tree that could be read.
    assert!(!env.home.exists() || fs::read_dir(&env.home).unwrap().all(|e| !e.unwrap().path().join("manifest.bin").exists()));
}