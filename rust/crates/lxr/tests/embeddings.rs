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
