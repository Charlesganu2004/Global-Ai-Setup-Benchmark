//! Source that is cheap to write and expensive to read: generated, minified or broken files
//! under the size limit of the walk. Each test has a time or a size bound that the cost per
//! symbol, not the cost of the file, decides; the bounds are many times the expected result
//! and many times below what the unbounded cost would be.

use lxr_core::{Lang, ParseGrade};
use lxr_extract::extract;
use std::time::{Duration, Instant};

const BOUND: Duration = Duration::from_secs(5);

fn timed<T>(work: impl FnOnce() -> T) -> (T, Duration) {
    let started = Instant::now();
    let result = work();
    (result, started.elapsed())
}

#[test]
fn symbols_sharing_one_long_line_do_not_hash_it_once_each() {
    let text = "function f(){g();}".repeat(40_000);
    let (facts, took) = timed(|| extract("min.js", Lang::JavaScript, &text));
    assert_eq!(facts.grade, ParseGrade::Ast);
    assert_eq!(facts.symbols.len(), 40_000);
    assert_eq!(facts.calls.len(), 40_000);
    assert!(facts.symbols.iter().all(|s| s.hash == facts.symbols[0].hash));
    assert!(took < BOUND, "40,000 definitions on one line took {took:?}");
}

#[test]
fn rust_symbols_sharing_one_long_line_do_not_hash_it_once_each() {
    let text = "fn f(){g();}".repeat(40_000);
    let (facts, took) = timed(|| extract("min.rs", Lang::Rust, &text));
    assert_eq!(facts.grade, ParseGrade::Ast);
    assert_eq!(facts.symbols.len(), 40_000);
    assert!(took < BOUND, "40,000 definitions on one line took {took:?}");
}

#[test]
fn the_hash_of_a_definition_follows_its_text() {
    let a = extract("a.js", Lang::JavaScript, "function f() { return 1; }\nfunction g() { return 2; }\n");
    let b = extract("a.js", Lang::JavaScript, "function f() { return 1; }\nfunction g() { return 3; }\n");
    assert_eq!(a.symbols[0].hash, b.symbols[0].hash);
    assert_ne!(a.symbols[1].hash, b.symbols[1].hash);
}

#[test]
fn the_hash_of_a_very_long_definition_still_follows_an_edit_at_its_end_and_a_change_of_length() {
    let body = |middle: &str, tail: &str| format!("function f() {{\n{}\n{middle}\n{}\n{tail}\n}}\n", "a();\n".repeat(30_000), "b();\n".repeat(30_000));
    let base = extract("long.js", Lang::JavaScript, &body("c();", "d();"));
    let tail = extract("long.js", Lang::JavaScript, &body("c();", "e();"));
    let longer = extract("long.js", Lang::JavaScript, &body("c();\nc();", "d();"));
    assert_eq!(base.symbols[0].start, 1);
    assert_ne!(base.symbols[0].hash, tail.symbols[0].hash, "an edit near the end changes the hash");
    assert_ne!(base.symbols[0].hash, longer.symbols[0].hash, "a longer definition changes the hash");
}

#[test]
fn unclosed_class_lines_keep_their_qualified_names_short() {
    let n = 8_000;
    let text = "class A {\n".repeat(n);
    let (facts, took) = timed(|| extract("A.java", Lang::Java, &text));
    assert_eq!(facts.symbols.len(), n);
    let longest = facts.symbols.iter().map(|s| s.qual.len()).max().unwrap_or(0);
    assert!(longest <= 512, "a qualified name of {longest} bytes");
    let total: usize = facts.symbols.iter().map(|s| s.qual.len() + s.sig.len() + s.name.len()).sum();
    assert!(total < n * 1_000, "{total} bytes of symbol text");
    assert!(took < BOUND, "{n} unclosed classes took {took:?}");
    // The nesting itself is still reported: each class sits in the one before it.
    assert_eq!(facts.symbols[0].parent, None);
    assert_eq!(facts.symbols[1].parent, Some(0));
    assert_eq!(facts.symbols[2].parent, Some(1));
}

#[test]
fn unclosed_namespace_statements_keep_the_names_after_them_short() {
    for (lang, path, head, tail) in [
        (Lang::CSharp, "A.cs", "namespace A;\n", "class C { }\n"),
        (Lang::Php, "A.php", "<?php\nnamespace A;\n", "class C { }\n"),
    ] {
        let mut text = String::from(head);
        text.push_str(&"namespace A;\n".repeat(8_000));
        text.push_str(tail);
        let (facts, took) = timed(|| extract(path, lang, &text));
        let class = facts.symbols.iter().find(|s| s.name == "C").expect("the class after the namespaces is found");
        assert!(class.qual.len() <= 512, "{path}: a qualified name of {} bytes", class.qual.len());
        assert!(class.qual.ends_with(".C"), "{path}: {}", class.qual);
        assert!(took < BOUND, "{path}: took {took:?}");
    }
}

#[test]
fn a_long_chain_of_python_calls_on_one_line_is_cheap() {
    let text = format!("x = a{}\n", ".m(1)".repeat(40_000));
    let (facts, took) = timed(|| extract("chain.py", Lang::Python, &text));
    assert_eq!(facts.grade, ParseGrade::Ast);
    assert_eq!(facts.calls.len(), 40_000);
    assert!(facts.calls.iter().all(|c| c.recv.chars().count() <= 60));
    assert!(took < BOUND, "a chain of 40,000 calls took {took:?}");
}
