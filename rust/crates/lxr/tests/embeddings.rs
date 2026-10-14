//! Search with an embedding model. These tests run only on a machine that has one; the
//! other test files hide the model so that their answers do not depend on the machine.

mod common;
use common::Env;

#[test]
fn a_model_gives_the_index_vectors_and_the_meaning_mode_results() {
    let Some(model) = lxr_text::Embedder::locate() else {
        eprintln!("no embedding model on this machine: test skipped");
        return;
    };
    let model = model.to_string_lossy().into_owned();
    let vars = [("LXR_MODEL_DIR", model.as_str())];
    let env = Env::with_project();

    let refresh = env.run_with(&["refresh"], &vars);
    assert_eq!(refresh.code, 0, "{}", refresh.err);
    assert!(refresh.out.contains("search ready (14 chunks, embeddings)"), "{}", refresh.out);
    assert!(!refresh.err.contains("no embedding model"), "{}", refresh.err);

    let meaning = env.run_with(&["find", "--meaning", "-k", "3", "read the configuration file"], &vars);
    assert_eq!(meaning.code, 0, "{}", meaning.err);
    assert!(meaning.out.starts_with("load_config app/core.py:4-6 (path)\n") || meaning.out.contains("load_config app/core.py:4-6"), "{}", meaning.out);

    let hybrid = env.run_with(&["find", "-k", "1", "read the configuration file"], &vars);
    assert!(hybrid.out.starts_with("load_config app/core.py:4-6 (path)\n"), "{}", hybrid.out);

    // Without the model the same index still answers by words.
    let words = env.run(&["find", "-k", "1", "read the configuration file"]);
    assert!(words.out.starts_with("load_config app/core.py:4-6 (path)\n"), "{}", words.out);
}

/// The text of the manifest of the project's index: it changes whenever the index is rebuilt.
fn manifest_of(env: &Env) -> Vec<u8> {
    std::fs::read(env.index_dir().join("manifest.bin")).unwrap()
}

#[test]
fn no_model_variable_builds_the_index_by_words_and_never_rebuilds_it() {
    let Some(model) = lxr_text::Embedder::locate() else {
        eprintln!("no embedding model on this machine: test skipped");
        return;
    };
    let model = model.to_string_lossy().into_owned();
    // The model is found on disk through LXR_MODEL_DIR; the variable still switches it off.
    let vars = [("LXR_MODEL_DIR", model.as_str()), ("PGR_NO_MODEL", "1")];
    let env = Env::with_project();

    let refresh = env.run_with(&["refresh"], &vars);
    assert_eq!(refresh.code, 0, "{}", refresh.err);
    assert!(refresh.out.contains("search ready (14 chunks, words only)"), "{}", refresh.out);
    assert!(refresh.err.contains("no embedding model found, search uses words only"), "{}", refresh.err);
    let built = manifest_of(&env);

    for args in [&["find", "-k", "1", "read the configuration file"][..], &["ask", "find:read the configuration file"], &["pack", "read the configuration file"]] {
        let run = env.run_with(args, &vars);
        assert_eq!(run.code, 0, "{args:?}: {}", run.err);
        assert!(run.out.contains("load_config"), "{args:?}: {}", run.out);
        assert!(!run.err.contains("embedding"), "{args:?}: {}", run.err);
    }
    // The meaning mode has nothing to work with, as on a machine without a model.
    let meaning = env.run_with(&["find", "--meaning", "read the configuration file"], &vars);
    let none = env.run(&["find", "--meaning", "read the configuration file"]);
    assert_eq!((meaning.code, meaning.out.as_str()), (none.code, none.out.as_str()), "{}", meaning.err);
    assert_eq!(manifest_of(&env), built, "a query does not rebuild an index that was built by words");
    // Without the variable the model is used again, and the index built by words is not rebuilt for it.
    let again = env.run_with(&["find", "-k", "1", "read the configuration file"], &[("LXR_MODEL_DIR", model.as_str())]);
    assert_eq!(again.code, 0, "{}", again.err);
    assert_eq!(manifest_of(&env), built);
}

#[test]
fn no_model_variable_leaves_an_index_with_vectors_alone_and_answers_by_words() {
    let Some(model) = lxr_text::Embedder::locate() else {
        eprintln!("no embedding model on this machine: test skipped");
        return;
    };
    let model = model.to_string_lossy().into_owned();
    let env = Env::with_project();
    let refresh = env.run_with(&["refresh"], &[("LXR_MODEL_DIR", model.as_str())]);
    assert!(refresh.out.contains("search ready (14 chunks, embeddings)"), "{}", refresh.out);
    let built = manifest_of(&env);
    let vars = [("LXR_MODEL_DIR", model.as_str()), ("PGR_NO_MODEL", "1")];
    for args in [&["find", "-k", "1", "read the configuration file"][..], &["ask", "find:read the configuration file"], &["pack", "read the configuration file"]] {
        let run = env.run_with(args, &vars);
        assert_eq!(run.code, 0, "{args:?}: {}", run.err);
        assert!(run.out.contains("load_config"), "{args:?}: {}", run.out);
    }
    assert_eq!(manifest_of(&env), built, "a query with the variable set does not rebuild the index");
    // The same variable at a rebuild: no vectors.
    let forced = env.run_with(&["refresh", "--force"], &vars);
    assert!(forced.out.contains("search ready (14 chunks, words only)"), "{}", forced.out);
}
