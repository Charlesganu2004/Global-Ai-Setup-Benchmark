//! The model compiled into the program. These tests exist only in a build with the
//! `embedded-model` feature (which needs `PGR_EMBED_MODEL_DIR`), and run against a machine on
//! which no model folder can be found: the fake user home and Hugging Face cache of the test
//! environment are empty, and `LXR_MODEL_DIR` is unset.
#![cfg(feature = "embedded-model")]

mod common;
use common::Env;
use std::fs;

#[test]
fn the_compiled_in_model_gives_the_index_vectors_without_any_model_folder() {
    let env = Env::with_project();
    let refresh = env.run(&["refresh"]);
    assert_eq!(refresh.code, 0, "{}", refresh.err);
    assert!(refresh.out.contains("search ready (14 chunks, embeddings)"), "{}", refresh.out);
    assert!(!refresh.err.contains("no embedding model"), "{}", refresh.err);

    // Nothing has changed, and the model is still found: no warning on the second run either.
    let again = env.run(&["refresh"]);
    assert_eq!(again.code, 0, "{}", again.err);
    assert!(!again.err.contains("no embedding model"), "{}", again.err);

    let meaning = env.run(&["find", "--meaning", "-k", "3", "read the configuration file"]);
    assert_eq!(meaning.code, 0, "{}", meaning.err);
    assert!(meaning.out.contains("load_config app/core.py:4-6"), "{}", meaning.out);
}

#[test]
fn the_vocabulary_of_the_compiled_in_model_is_cached_by_the_hash_of_its_bytes() {
    let env = Env::with_project();
    assert_eq!(env.run(&["refresh"]).code, 0);
    let cache = env.home.join("model-cache");
    let names: Vec<String> = fs::read_dir(&cache).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
    assert_eq!(names.len(), 1, "{names:?}");
    assert!(names[0].starts_with("vocab-bytes-") && names[0].ends_with(".bin"), "{names:?}");
    // A later run uses the file and does not add another.
    assert_eq!(env.run(&["find", "--meaning", "-k", "1", "read the configuration file"]).code, 0);
    assert_eq!(fs::read_dir(&cache).unwrap().count(), 1);
}

#[test]
fn a_model_folder_that_holds_no_model_does_not_hide_the_compiled_in_one() {
    let env = Env::with_project();
    let empty = env.base.join("empty-model-folder");
    fs::create_dir_all(&empty).unwrap();
    let vars = [("LXR_MODEL_DIR", empty.to_str().unwrap())];
    let refresh = env.run_with(&["refresh"], &vars);
    assert_eq!(refresh.code, 0, "{}", refresh.err);
    assert!(refresh.out.contains("search ready (14 chunks, embeddings)"), "{}", refresh.out);
    assert!(!refresh.err.contains("no embedding model"), "{}", refresh.err);
}
