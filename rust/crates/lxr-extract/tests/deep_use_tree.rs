//! A file nested past the parsers' depth limit is read by the token-based extractor, which
//! must not overflow the stack either. A stack overflow cannot be caught and ends the whole
//! process, so this test has a binary of its own: a failure here cannot hide other results.

use lxr_core::{Lang, ParseGrade};
use lxr_extract::extract;

/// `use a::{a::{ ... a }};` nested `depth` levels, then a function on the next line.
fn nested_use(depth: usize) -> String {
    let mut text = String::from("use ");
    for _ in 0..depth {
        text.push_str("a::{");
    }
    text.push('a');
    for _ in 0..depth {
        text.push('}');
    }
    text.push_str(";\nfn f() {}\n");
    text
}

#[test]
fn a_use_tree_past_the_depth_limit_is_read_without_overflowing_the_stack() {
    // One more level than the Rust parser accepts, so the file goes to the token-based
    // extractor. It runs on a thread with a 1 MiB stack, smaller than the stack of a thread
    // pool worker, where one stack frame per level would not fit.
    let text = nested_use(20_005);
    let reader = std::thread::Builder::new()
        .stack_size(1 << 20)
        .spawn(move || extract("deep.rs", Lang::Rust, &text))
        .expect("a thread with a small stack starts");
    let facts = reader.join().expect("the file is read without a panic");
    assert_eq!(facts.grade, ParseGrade::Lexical);
    let f = facts.symbols.iter().find(|s| s.name == "f").expect("the function after the use tree is found");
    assert_eq!((f.start, f.end), (2, 2));
}

#[test]
fn deeply_nested_brackets_are_read_in_every_language_without_overflowing_the_stack() {
    // The parsers of JavaScript, TypeScript and Rust refuse a file this deep and leave it to
    // the token-based extractor, as do all the other languages without a parser. Python is
    // left out: its parser has no depth limit, and is covered by its own tests.
    let langs = [
        (Lang::JavaScript, "a.js"),
        (Lang::Jsx, "a.jsx"),
        (Lang::TypeScript, "a.ts"),
        (Lang::Tsx, "a.tsx"),
        (Lang::Rust, "a.rs"),
        (Lang::Go, "a.go"),
        (Lang::Java, "A.java"),
        (Lang::C, "a.c"),
        (Lang::Cpp, "a.cpp"),
        (Lang::CSharp, "A.cs"),
        (Lang::Kotlin, "a.kt"),
        (Lang::Swift, "a.swift"),
        (Lang::Ruby, "a.rb"),
        (Lang::Php, "a.php"),
        (Lang::Shell, "a.sh"),
    ];
    let depth = 30_000;
    let shapes = [
        ("{", "}", "braces"),
        ("(", ")", "parentheses"),
        ("[", "]", "square brackets"),
        ("f(", ")", "calls"),
    ];
    for (lang, path) in langs {
        for (open, close, what) in shapes {
            let text = format!("<?php\n{}x{}\nfn g() {{}}\n", open.repeat(depth), close.repeat(depth));
            let reader = std::thread::Builder::new()
                .stack_size(1 << 20)
                .spawn(move || extract(path, lang, &text))
                .expect("a thread with a small stack starts");
            let facts = reader.join().unwrap_or_else(|_| panic!("{path}: {depth} nested {what} panicked"));
            assert!(facts.symbols.iter().all(|s| s.start <= s.end), "{path}: {what}");
        }
    }
}
