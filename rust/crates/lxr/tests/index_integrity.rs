//! The index must match the files. A query that cannot make the index current stops with
//! exit 1 and says why; it never answers from an index that does not match. The pieces of
//! an index are read as one generation, never a mix of two.

mod common;
use common::{expect, Env};
use std::fs;
use std::process::Stdio;
use std::time::{Duration, SystemTime};

const CLEAN_NAME: &str = "\n\ndef clean_name(text):\n    return slugify(text)\n";

/// Adds a function that calls `slugify`, so that `callers slugify` has a new answer.
fn add_clean_name(env: &Env) {
    let mut util = env.read("app/util.py");
    util.push_str(CLEAN_NAME);
    env.write("app/util.py", &util);
}

/// Sets the modified time of a file.
fn set_time(path: &std::path::Path, time: SystemTime) {
    let file = fs::OpenOptions::new().write(true).open(path).unwrap();
    file.set_modified(time).unwrap();
}

#[test]
fn a_lock_held_too_long_is_an_error_and_not_a_stale_answer() {
    let env = Env::with_project();
    expect(&env.run(&["callers", "slugify"]), "0 callers of slugify\n");
    add_clean_name(&env);
    // Another process holds the lock right now.
    let lock = env.index_dir().join("refresh.lock");
    fs::write(&lock, "999999\n").unwrap();

    let run = env.run_with(&["callers", "slugify"], &[("PGR_LOCK_WAIT_MS", "300")]);
    assert_eq!(run.code, 1, "stdout was: {}", run.out);
    assert_eq!(run.out, "", "a query that cannot make the index current must not answer");
    assert!(run.err.starts_with("pgr: ") && run.err.contains("refresh"), "{}", run.err);
    assert!(lock.exists(), "a lock another process holds is not ours to remove");

    let refresh = env.run_with(&["refresh"], &[("PGR_LOCK_WAIT_MS", "300")]);
    assert_eq!(refresh.code, 1, "{}", refresh.out);

    // PGR_NO_REFRESH asks for the index as it is: the old answer is allowed then.
    expect(&env.run_with(&["callers", "slugify"], &[("PGR_NO_REFRESH", "1")]), "0 callers of slugify\n");

    // Once the lock is free the query is current again.
    fs::remove_file(&lock).unwrap();
    expect(&env.run(&["callers", "slugify"]), "1 caller of slugify:\nclean_name app/util.py:11-12 @12\n");
}

#[test]
fn a_lock_that_is_released_in_time_is_waited_for() {
    let env = Env::with_project();
    expect(&env.run(&["callers", "slugify"]), "0 callers of slugify\n");
    add_clean_name(&env);
    let lock = env.index_dir().join("refresh.lock");
    fs::write(&lock, "999999\n").unwrap();
    let remover = std::thread::spawn({
        let lock = lock.clone();
        move || {
            std::thread::sleep(Duration::from_millis(600));
            fs::remove_file(lock).unwrap();
        }
    });
    let run = env.run_with(&["callers", "slugify"], &[("PGR_LOCK_WAIT_MS", "20000")]);
    remover.join().unwrap();
    expect(&run, "1 caller of slugify:\nclean_name app/util.py:11-12 @12\n");
}

#[test]
fn several_processes_refreshing_at_once_leave_one_good_index() {
    let env = Env::with_project();
    let children: Vec<_> = (0..5)
        .map(|_| {
            let mut cmd = env.command();
            cmd.arg("--root").arg(&env.proj).args(["refresh", "--force"]);
            cmd.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap()
        })
        .collect();
    for child in children {
        let out = child.wait_with_output().unwrap();
        assert_eq!(out.status.code(), Some(0), "{}", String::from_utf8_lossy(&out.stderr));
    }
    let status = env.run(&["status"]);
    assert!(status.out.contains("graph ready (12 symbols"), "{}", status.out);
    expect(&env.run(&["def", "slugify"]), "slugify app/util.py:1-3 (text)\n");
    assert!(!env.index_dir().join("refresh.lock").exists(), "the lock must be released");
}

#[test]
fn an_expired_lock_is_taken_over_by_exactly_one_of_several_processes() {
    let env = Env::with_project();
    env.run(&["refresh"]);
    env.write("app/extra.py", "def extra_one():\n    return 1\n");
    let lock = env.index_dir().join("refresh.lock");
    fs::write(&lock, "999999\n").unwrap();
    set_time(&lock, SystemTime::now() - Duration::from_secs(700));
    let children: Vec<_> = (0..5)
        .map(|_| {
            let mut cmd = env.command();
            cmd.arg("--root").arg(&env.proj).args(["def", "extra_one"]);
            cmd.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap()
        })
        .collect();
    let mut refreshed = 0;
    for child in children {
        let out = child.wait_with_output().unwrap();
        let err = String::from_utf8_lossy(&out.stderr).into_owned();
        assert_eq!(out.status.code(), Some(0), "{err}");
        assert_eq!(String::from_utf8_lossy(&out.stdout), "extra_one app/extra.py:1-2 ()\n", "{err}");
        if err.contains("index refreshed") {
            refreshed += 1;
        }
    }
    assert_eq!(refreshed, 1, "one process builds, the others find the index current");
    assert!(!lock.exists());
}

#[test]
fn a_graph_from_another_generation_is_not_read_with_this_manifest() {
    let env = Env::with_project();
    env.run(&["refresh"]);
    // The same project with one more function, indexed on its own.
    let other = Env::with_project();
    other.write("app/extra.py", "def extra_one():\n    return 1\n");
    assert_eq!(other.run(&["refresh"]).code, 0);
    fs::copy(other.index_dir().join("graph.bin"), env.index_dir().join("graph.bin")).unwrap();

    // As it is, the index is not one index: there is nothing to answer from.
    let blocked = env.run_with(&["def", "extra_one"], &[("PGR_NO_REFRESH", "1")]);
    assert_eq!(blocked.code, 1, "stdout was: {}", blocked.out);
    assert_eq!(blocked.out, "");
    assert!(blocked.err.starts_with("pgr: ") && blocked.err.contains("refresh --force"), "{}", blocked.err);

    // A normal query builds the index again and answers from the files.
    let healed = env.run(&["def", "extra_one"]);
    expect(&healed, "extra_one: not found\n");
    assert!(healed.err.starts_with("pgr: index built in "), "{}", healed.err);
    assert_eq!(env.run(&["def", "extra_one"]).err, "");
}

#[test]
fn a_search_folder_from_another_generation_is_not_read_with_this_manifest() {
    let env = Env::with_project();
    env.run(&["refresh"]);
    let other = Env::with_project();
    other.write("app/extra.py", "def extra_one():\n    return 1\n");
    assert_eq!(other.run(&["refresh"]).code, 0);
    let search = env.index_dir().join("search");
    fs::remove_dir_all(&search).unwrap();
    for entry in fs::read_dir(other.index_dir().join("search")).unwrap() {
        let entry = entry.unwrap();
        fs::create_dir_all(&search).unwrap();
        fs::copy(entry.path(), search.join(entry.file_name())).unwrap();
    }
    let blocked = env.run_with(&["find", "extra_one"], &[("PGR_NO_REFRESH", "1")]);
    assert_eq!(blocked.code, 1, "stdout was: {}", blocked.out);
    let healed = env.run(&["find", "extra_one"]);
    assert_eq!(healed.code, 0, "{}", healed.err);
    assert!(!healed.out.contains("extra.py"), "{}", healed.out);
}

/// A way to make a file unreadable for the program while the test can still list and stat it.
struct Denied {
    #[cfg(windows)]
    _handle: fs::File,
    #[cfg(unix)]
    path: std::path::PathBuf,
}

#[cfg(windows)]
fn deny_read(path: &std::path::Path) -> Option<Denied> {
    use std::os::windows::fs::OpenOptionsExt;
    // A file opened without any sharing cannot be opened by another process.
    let handle = fs::OpenOptions::new().read(true).share_mode(0).open(path).ok()?;
    Some(Denied { _handle: handle })
}

#[cfg(unix)]
fn deny_read(path: &std::path::Path) -> Option<Denied> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o000)).ok()?;
    if fs::read(path).is_ok() {
        // Running as a user that ignores permissions: nothing can be denied.
        fs::set_permissions(path, fs::Permissions::from_mode(0o644)).ok()?;
        return None;
    }
    Some(Denied { path: path.to_path_buf() })
}

#[cfg(unix)]
impl Drop for Denied {
    fn drop(&mut self) {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(&self.path, fs::Permissions::from_mode(0o644));
    }
}

#[test]
fn a_source_file_that_cannot_be_read_stops_the_build_and_does_not_vanish_from_the_index() {
    let env = Env::with_project();
    env.run(&["refresh"]);
    let util = env.proj.join("app/util.py");
    env.write("app/util.py", "def slugify(text):\n    return text.lower() + 'x'\n\n\ndef extra_two():\n    return 3\n");
    let Some(denied) = deny_read(&util) else {
        eprintln!("cannot deny reading here: test skipped");
        return;
    };
    let run = env.run(&["def", "slugify"]);
    assert_eq!(run.code, 1, "stdout was: {}", run.out);
    assert_eq!(run.out, "");
    assert!(run.err.starts_with("pgr: ") && run.err.contains("app/util.py"), "{}", run.err);
    // The index that was there is still the one on disk.
    let status = env.run(&["status"]);
    assert!(status.out.contains("graph ready (12 symbols"), "{}", status.out);
    drop(denied);
    let after = env.run(&["def", "extra_two"]);
    assert!(after.out.starts_with("extra_two app/util.py:"), "{}{}", after.out, after.err);
}

#[cfg(windows)]
#[test]
fn a_build_that_fails_half_way_leaves_no_mixed_index_behind() {
    use std::os::windows::fs::OpenOptionsExt;
    let env = Env::with_project();
    env.run(&["refresh"]);
    let original = env.read("app/util.py");
    // Every file of the search index is held open, so the folder cannot be replaced.
    let held: Vec<fs::File> = fs::read_dir(env.index_dir().join("search"))
        .unwrap()
        .map(|entry| fs::OpenOptions::new().read(true).share_mode(0).open(entry.unwrap().path()).unwrap())
        .collect();
    assert!(!held.is_empty());
    env.write("app/util.py", &format!("{original}\n\ndef ynew():\n    return 2\n"));
    let failed = env.run(&["def", "ynew"]);
    assert_eq!(failed.code, 1, "stdout was: {} stderr was: {}", failed.out, failed.err);
    assert_eq!(failed.out, "");
    drop(held);

    // The file is as it was when the index was made; what a half-finished build wrote for
    // the other version must not come back as the answer.
    env.write("app/util.py", &original);
    let after = env.run(&["def", "ynew"]);
    expect(&after, "ynew: not found\n");
    expect(&env.run(&["def", "slugify"]), "slugify app/util.py:1-3 (text)\n");
}
