//! The files of an index are stored packed and losslessly (`lxr_core::pack`). A damaged file
//! is found, the index is built again, and the answers are the ones of an undamaged index:
//! a damaged file is never read as other values and never makes the program panic.

mod common;
use common::{expect, Env};
use lxr_core::pack::unpack;
use std::fs;

const QUERIES: [&[&str]; 5] = [
    &["def", "slugify"],
    &["callers", "read_text"],
    &["about", "load_config"],
    &["find", "-k", "3", "read the configuration file"],
    &["ask", "def:slugify", "callers:parse_text", "find:turn text into settings"],
];

/// The model folder of this machine, if it has one; the vectors file exists only with it.
fn model_dir() -> Option<String> {
    lxr_text::Embedder::locate().map(|p| p.to_string_lossy().into_owned())
}

/// The answers to the queries: exit code and standard output of each.
fn answers(env: &Env, vars: &[(&str, &str)]) -> Vec<(i32, String)> {
    QUERIES
        .iter()
        .map(|query| {
            let run = env.run_with(query, vars);
            assert!(!run.err.contains("panicked"), "{query:?}: {}", run.err);
            (run.code, run.out)
        })
        .collect()
}

/// A project with a finished index, built with the model when the machine has one.
fn built(model: &Option<String>) -> Env {
    let env = Env::with_project();
    let vars = vars_of(model);
    let refresh = env.run_with(&["refresh"], &vars);
    assert_eq!(refresh.code, 0, "{}", refresh.err);
    env
}

fn vars_of(model: &Option<String>) -> Vec<(&str, &str)> {
    model.iter().map(|m| ("LXR_MODEL_DIR", m.as_str())).collect()
}

#[test]
fn every_file_of_an_index_is_stored_packed_and_the_format_number_is_3() {
    let model = model_dir();
    let env = built(&model);
    let dir = env.index_dir();
    for name in ["manifest.bin", "facts.bin", "graph.bin", "search/chunks.bin", "search/bm25.bin"] {
        let bytes = fs::read(dir.join(name)).unwrap();
        assert_eq!(&bytes[..4], b"LXZ1", "{name} is not in the container");
    }
    // The manifest begins with its format number.
    let manifest = unpack(&fs::read(dir.join("manifest.bin")).unwrap()).unwrap();
    assert_eq!(manifest[0], 3, "the index format number");
    if model.is_some() {
        let vectors = fs::read(dir.join("search/vectors.bin")).unwrap();
        assert_eq!(&vectors[..8], b"LXRVEC02", "the vectors are not packed");
    } else {
        eprintln!("no embedding model on this machine: the vectors file was not checked");
    }
}

#[test]
fn packed_files_answer_exactly_as_the_files_of_a_rebuild_do() {
    let model = model_dir();
    let vars = vars_of(&model);
    let env = built(&model);
    let first = answers(&env, &vars);
    assert!(first.iter().all(|(code, out)| *code == 0 && !out.is_empty()), "{first:?}");
    expect(&env.run_with(&["def", "slugify"], &vars), "slugify app/util.py:1-3 (text)\n");
    // A forced rebuild writes the files again and gives the same answers.
    assert_eq!(env.run_with(&["refresh", "--force"], &vars).code, 0);
    assert_eq!(answers(&env, &vars), first);
}

/// One way to damage a file.
enum Damage {
    FlipMiddle,
    FlipLast,
    CutInHalf,
    Empty,
}

impl Damage {
    fn apply(&self, bytes: &[u8]) -> Vec<u8> {
        let mut out = bytes.to_vec();
        match self {
            Damage::FlipMiddle => out[bytes.len() / 2] ^= 0x10,
            Damage::FlipLast => *out.last_mut().unwrap() ^= 0x01,
            Damage::CutInHalf => out.truncate(bytes.len() / 2),
            Damage::Empty => out.clear(),
        }
        out
    }
}

#[test]
fn a_damaged_file_is_found_and_the_index_is_built_again_with_the_same_answers() {
    let model = model_dir();
    let vars = vars_of(&model);
    let mut files = vec!["manifest.bin", "graph.bin", "search/chunks.bin", "search/bm25.bin"];
    if model.is_some() {
        files.push("search/vectors.bin");
    }
    for file in files {
        for damage in [Damage::FlipMiddle, Damage::FlipLast, Damage::CutInHalf, Damage::Empty] {
            let env = built(&model);
            let baseline = answers(&env, &vars);
            let path = env.index_dir().join(file);
            let bytes = fs::read(&path).unwrap();
            fs::write(&path, damage.apply(&bytes)).unwrap();

            // As it is, the index is refused, or it answers as it always did: never anything else.
            let mut blocked_vars = vars.clone();
            blocked_vars.push(("PGR_NO_REFRESH", "1"));
            let blocked = env.run_with(QUERIES[0], &blocked_vars);
            assert!(!blocked.err.contains("panicked"), "{file}: {}", blocked.err);
            if blocked.code == 0 {
                assert_eq!(blocked.out, baseline[0].1, "{file}: a damaged file gave a different answer");
            } else {
                assert_eq!((blocked.code, blocked.out.as_str()), (1, ""), "{file}: {}", blocked.err);
            }

            // A normal query builds the index again; every answer is the baseline's.
            let healed = env.run_with(QUERIES[0], &vars);
            assert_eq!((healed.code, &healed.out), (0, &baseline[0].1), "{file}: {}", healed.err);
            if blocked.code != 0 {
                assert!(healed.err.starts_with("pgr: index built in "), "{file}: {}", healed.err);
            }
            assert_eq!(answers(&env, &vars), baseline, "{file}");
            assert_eq!(env.run_with(QUERIES[0], &vars).err, "", "{file}: the index was not left whole");
        }
    }
}

#[test]
fn a_damaged_facts_file_is_not_trusted_when_the_index_is_refreshed() {
    let model = model_dir();
    let vars = vars_of(&model);
    let env = built(&model);
    let path = env.index_dir().join("facts.bin");
    let mut bytes = fs::read(&path).unwrap();
    let middle = bytes.len() / 2;
    bytes[middle] ^= 0x20;
    fs::write(&path, &bytes).unwrap();
    // A source file changes, so the facts of the others would be reused if they were trusted.
    let util = env.read("app/util.py");
    env.write("app/util.py", &format!("{util}\n\ndef clean_name(text):\n    return slugify(text)\n"));
    let after = answers(&env, &vars);

    let fresh = Env::with_project();
    fresh.write("app/util.py", &env.read("app/util.py"));
    assert_eq!(fresh.run_with(&["refresh"], &vars).code, 0);
    assert_eq!(after, answers(&fresh, &vars), "the refresh used a damaged facts file");
    expect(&env.run_with(&["callers", "slugify"], &vars), "1 caller of slugify:\nclean_name app/util.py:11-12 @12\n");
}

#[test]
fn an_index_of_the_earlier_format_is_built_again_and_never_read() {
    let model = model_dir();
    let vars = vars_of(&model);
    let env = built(&model);
    let baseline = answers(&env, &vars);
    let dir = env.index_dir();
    // The files as 0.2 wrote them: postcard bytes with no container, the manifest saying format 2.
    for name in ["manifest.bin", "graph.bin", "facts.bin", "search/chunks.bin"] {
        let path = dir.join(name);
        let mut plain = unpack(&fs::read(&path).unwrap()).unwrap();
        if name == "manifest.bin" {
            assert_eq!(plain[0], 3);
            plain[0] = 2;
        }
        fs::write(&path, plain).unwrap();
    }
    let blocked = env.run_with(QUERIES[0], &[("PGR_NO_REFRESH", "1")]);
    assert_eq!((blocked.code, blocked.out.as_str()), (1, ""), "{}", blocked.err);
    let healed = env.run_with(QUERIES[0], &vars);
    assert_eq!(healed.code, 0, "{}", healed.err);
    assert!(healed.err.starts_with("pgr: index built in "), "{}", healed.err);
    assert_eq!(answers(&env, &vars), baseline);
}
