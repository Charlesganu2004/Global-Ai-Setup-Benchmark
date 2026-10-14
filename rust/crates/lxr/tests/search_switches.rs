//! The switches of the search (`PGR_SEARCH_WEIGHTS`, `PGR_SEARCH_REFINE`, `PGR_SEARCH_VECTORS`,
//! `PGR_SEARCH_GRAPH`) are off unless set: with none set, or with each at its default, every command
//! prints what it printed before. A switch that cannot be understood is a usage error, and each
//! switch that is on does what its README row says.

mod common;
use common::Env;
use std::fs;
use std::path::{Path, PathBuf};

/// A project of 40 small functions in which the word `theme0` to `theme7` is in five each, and a
/// function `compute` that only a test calls.
fn themed() -> Env {
    let env = Env::new();
    for file in 0..4 {
        let mut text = String::new();
        for n in 0..10 {
            let id = file * 10 + n;
            text.push_str(&format!("def handler_{id}(value):\n    \"\"\"Handle theme{} input.\"\"\"\n    return value + {id}\n\n\n", id % 8));
        }
        env.write(&format!("src/part{file}.py"), &text);
    }
    env.write("src/geometry.py", "def compute(radius):\n    return 3.14159 * radius * radius\n");
    env.write(
        "tests/test_geometry.py",
        "from src.geometry import compute\n\n\ndef test_circle_area_is_pi_r_squared():\n    assert compute(2) > 12\n",
    );
    env
}

fn files_named(dir: &Path, prefix: &str) -> Vec<PathBuf> {
    let mut found = Vec::new();
    for entry in fs::read_dir(dir).unwrap().filter_map(Result::ok) {
        let path = entry.path();
        if path.is_dir() {
            found.extend(files_named(&path, prefix));
        } else if path.file_name().is_some_and(|n| n.to_string_lossy().starts_with(prefix)) {
            found.push(path);
        }
    }
    found
}

const QUERIES: [&[&str]; 4] = [
    &["find", "-k", "5", "handle theme3 input"],
    &["find", "--words", "-k", "5", "circle area pi squared"],
    &["ask", "def:compute", "find:handle theme5 input"],
    &["pack", "handle theme2 input and compute the circle area"],
];

#[test]
fn switches_unset_or_at_their_defaults_change_nothing() {
    let env = themed();
    let plain: Vec<(i32, String)> = QUERIES
        .iter()
        .map(|q| {
            let run = env.run(q);
            assert_eq!(run.code, 0, "{q:?}: {}", run.err);
            (run.code, run.out)
        })
        .collect();
    assert!(plain.iter().all(|(_, out)| !out.trim().is_empty()));
    let defaults = [
        ("PGR_SEARCH_WEIGHTS", "names=1,files=1,words=1,meaning=1,k=60,doc=0,refine=0.5,terms=8,repo=0.5"),
        ("PGR_SEARCH_REFINE", "0"),
        ("PGR_SEARCH_VECTORS", "model"),
        ("PGR_SEARCH_GRAPH", "0"),
    ];
    for query in QUERIES {
        let same = env.run_with(query, &defaults);
        let before = &plain[QUERIES.iter().position(|q| *q == query).unwrap()];
        assert_eq!((same.code, &same.out), (before.0, &before.1), "{query:?}");
    }
    // Empty variables are the same as unset ones.
    let empty = [("PGR_SEARCH_WEIGHTS", ""), ("PGR_SEARCH_REFINE", ""), ("PGR_SEARCH_VECTORS", ""), ("PGR_SEARCH_GRAPH", "")];
    assert_eq!(env.run_with(QUERIES[0], &empty).out, plain[0].1);
    // No default search leaves the vectors of the repository behind.
    assert!(files_named(&env.home, "repo1-").is_empty());
}

#[test]
fn a_switch_that_cannot_be_understood_is_a_usage_error() {
    let env = themed();
    for (name, value, needle) in [
        ("PGR_SEARCH_WEIGHTS", "nmes=1", "unknown weight 'nmes'"),
        ("PGR_SEARCH_WEIGHTS", "k=0", "above 0"),
        ("PGR_SEARCH_WEIGHTS", "names", "not key=value"),
        ("PGR_SEARCH_REFINE", "many", "PGR_SEARCH_REFINE"),
        ("PGR_SEARCH_REFINE", "51", "PGR_SEARCH_REFINE"),
        ("PGR_SEARCH_VECTORS", "neural", "PGR_SEARCH_VECTORS"),
        ("PGR_SEARCH_GRAPH", "off", "PGR_SEARCH_GRAPH"),
        ("PGR_SEARCH_GRAPH", "false", "PGR_SEARCH_GRAPH"),
        ("PGR_SEARCH_GRAPH", "no", "PGR_SEARCH_GRAPH"),
        ("PGR_SEARCH_GRAPH", "2", "PGR_SEARCH_GRAPH"),
    ] {
        let run = env.run_with(&["find", "handle theme3 input"], &[(name, value)]);
        assert_eq!(run.code, 2, "{name}={value}: {}", run.err);
        assert!(run.out.is_empty(), "{name}={value}: {}", run.out);
        assert!(run.err.contains(needle), "{name}={value}: {}", run.err);
    }
}

#[test]
fn weights_are_taken_and_keep_the_hits() {
    let env = themed();
    let plain = env.run(&["find", "-k", "8", "handle theme3 input"]);
    let stems = env.run_with(&["find", "-k", "8", "handle theme3 input"], &[("PGR_SEARCH_WEIGHTS", "names=0,files=0,k=1")]);
    assert_eq!((plain.code, stems.code), (0, 0));
    // The weights move scores, not what is found.
    let rows = |text: &str| -> Vec<String> { text.lines().filter(|l| !l.starts_with("  ")).map(str::to_string).collect() };
    let (mut a, mut b) = (rows(&plain.out), rows(&stems.out));
    a.sort();
    b.sort();
    assert!(!a.is_empty());
    assert_eq!(a.len(), b.len());
    // A weight that is read changes the answer: with the weight of the words at 0 the word list
    // adds nothing, so a search by words finds none of the five functions that hold the word.
    let query = ["find", "--words", "-k", "5", "theme3"];
    let found = env.run(&query);
    assert_eq!(rows(&found.out).len(), 5, "{}", found.out);
    let silenced = env.run_with(&query, &[("PGR_SEARCH_WEIGHTS", "words=0")]);
    assert_eq!((silenced.code, silenced.out.trim()), (0, "none"), "{}", silenced.err);
    // The same weight stated at its default value changes nothing.
    let stated = env.run_with(&query, &[("PGR_SEARCH_WEIGHTS", "words=1")]);
    assert_eq!(stated.out, found.out);
}

#[test]
fn a_refinement_pass_brings_in_a_hit_that_shares_a_rare_word_with_the_best_hit() {
    let env = themed();
    // `parse_quartz` is the only function that holds "quartz". `tokenise_zebra` does not, but it
    // shares the rare word "zebra" with it.
    env.write("src/quartz.py", "def parse_quartz(text):\n    \"\"\"Quartz parsing with the zebra tokenizer.\"\"\"\n    return text\n");
    env.write("src/zebra.py", "def tokenise_zebra(text):\n    \"\"\"Split text with the zebra tokenizer.\"\"\"\n    return text.split()\n");
    let names = |vars: &[(&str, &str)]| -> Vec<String> {
        let run = env.run_with(&["find", "--words", "-k", "5", "quartz"], vars);
        assert_eq!(run.code, 0, "{}", run.err);
        run.out.lines().filter(|l| !l.starts_with("  ")).map(|l| l.split(' ').next().unwrap().to_string()).collect()
    };
    // Without the pass the search finds the one function that holds the word, and nothing else.
    assert_eq!(names(&[]), ["parse_quartz"]);
    assert_eq!(names(&[("PGR_SEARCH_REFINE", "0")]), ["parse_quartz"]);
    // The pass learns the rare words of the best hit and fuses a second list in: the best hit
    // stays first, and the function that only shares a rare word with it comes in below.
    let refined = names(&[("PGR_SEARCH_REFINE", "1")]);
    assert_eq!(refined[0], "parse_quartz", "{refined:?}");
    assert!(refined.contains(&"tokenise_zebra".to_string()), "{refined:?}");
}

#[test]
fn vectors_made_from_the_repository_give_a_meaning_channel_without_a_model() {
    let env = themed();
    // No model in the tests: meaning alone finds nothing, repository vectors find the five
    // functions that hold the word.
    let none = env.run(&["find", "--meaning", "-k", "10", "theme3"]);
    assert_eq!(none.out.trim(), "none");
    let repo = [("PGR_SEARCH_VECTORS", "repo")];
    let found = env.run_with(&["find", "--meaning", "-k", "10", "theme3"], &repo);
    assert_eq!(found.code, 0, "{}", found.err);
    let mut handlers: Vec<&str> = found.out.lines().filter(|l| l.starts_with("handler_")).map(|l| l.split(' ').next().unwrap()).collect();
    assert!(handlers.len() >= 5, "{}", found.out);
    handlers.truncate(5);
    handlers.sort();
    assert_eq!(handlers, vec!["handler_11", "handler_19", "handler_27", "handler_3", "handler_35"], "{}", found.out);
    // The index was built without the switch, so nothing was kept beside it, and the answer is the same
    // once the index is built with the switch on and keeps the vectors.
    assert!(files_named(&env.home, "repo1-").is_empty());
    let built = env.run_with(&["refresh", "--force"], &repo);
    assert_eq!(built.code, 0, "{}", built.err);
    let kept = files_named(&env.home, "repo1-");
    assert_eq!(kept.len(), 1, "{kept:?}");
    let again = env.run_with(&["find", "--meaning", "-k", "10", "theme3"], &repo);
    assert_eq!(again.out, found.out);
    // `both` with no model is the repository's vectors and the words.
    let both = env.run_with(&["find", "-k", "10", "theme3"], &[("PGR_SEARCH_VECTORS", "both")]);
    assert_eq!(both.code, 0, "{}", both.err);
    assert!(both.out.contains("handler_3 "));
}

#[test]
fn a_test_hit_is_followed_by_the_function_it_calls() {
    let env = themed();
    let query = ["find", "--words", "-k", "5", "circle area pi squared"];
    let plain = env.run(&query);
    assert!(plain.out.contains("test_circle_area_is_pi_r_squared"), "{}", plain.out);
    assert!(!plain.out.contains("compute "), "{}", plain.out);
    let graph = env.run_with(&query, &[("PGR_SEARCH_GRAPH", "1")]);
    assert_eq!(graph.code, 0, "{}", graph.err);
    let rows: Vec<&str> = graph.out.lines().filter(|l| !l.starts_with("  ")).collect();
    let at = rows.iter().position(|r| r.starts_with("test_circle_area_is_pi_r_squared ")).expect("the test is a hit");
    assert!(rows.get(at + 1).is_some_and(|r| r.starts_with("compute src/geometry.py:")), "{}", graph.out);
    // The same switch with the graph engine off does nothing and does not fail.
    let off = env.run_with(&["--engines", "search", "find", "--words", "-k", "5", "circle area pi squared"], &[("PGR_SEARCH_GRAPH", "1")]);
    assert_eq!(off.code, 0, "{}", off.err);
    assert_eq!(off.out, plain.out);
}
