//! Which extractor a file goes through, seen from the crate's public functions.

use lxr_core::{Lang, ParseGrade, SymKind};
use lxr_extract::{extract, extract_path};

#[test]
fn python_that_parses_is_graded_ast() {
    let f = extract("pkg/a.py", Lang::Python, "def f():\n    return g()\n\nMAX = 3\n");
    assert_eq!(f.grade, ParseGrade::Ast);
    let names: Vec<(&str, SymKind, u32, u32)> = f.symbols.iter().map(|s| (s.name.as_str(), s.kind, s.start, s.end)).collect();
    assert_eq!(names, [("f", SymKind::Function, 1, 2), ("MAX", SymKind::Const, 4, 4)]);
    assert_eq!(f.calls.len(), 1);
    assert_eq!(f.calls[0].name, "g");
    assert_eq!(f.calls[0].caller, Some(0));
}

#[test]
fn python_the_parser_rejects_falls_back_to_tokens_and_says_so() {
    let source = "class A:\n    def m(self):\n        print 'two'\n        return self.go(1)\n\ndef top(:\n    pass\n";
    let f = extract("old.py", Lang::Python, source);
    assert_eq!(f.grade, ParseGrade::Lexical);
    let found: Vec<(String, u32)> = f.symbols.iter().map(|s| (s.qual.clone(), s.start)).collect();
    assert_eq!(found, [("A".to_string(), 1), ("A.m".to_string(), 2), ("top".to_string(), 6)]);
    assert!(f.calls.iter().any(|c| c.name == "go" && c.recv == "self"));
}

#[test]
fn languages_without_a_parser_are_graded_lexical() {
    let samples = [
        (Lang::Go, "a.go", "package p\n\nfunc F() { g() }\n", "F"),
        (Lang::Java, "A.java", "class A {\n  void f() { g(); }\n}\n", "A"),
        (Lang::C, "a.c", "int f(void) { return g(); }\n", "f"),
        (Lang::Cpp, "a.cpp", "struct S { int f() { return g(); } };\n", "S"),
        (Lang::CSharp, "A.cs", "class A { void F() { G(); } }\n", "A"),
        (Lang::Kotlin, "a.kt", "fun f() { g() }\n", "f"),
        (Lang::Swift, "a.swift", "func f() { g() }\n", "f"),
        (Lang::Ruby, "a.rb", "def f\n  g()\nend\n", "f"),
        (Lang::Php, "a.php", "<?php\nfunction f() { g(); }\n", "f"),
        (Lang::Shell, "a.sh", "f() {\n  :\n}\n", "f"),
    ];
    for (lang, path, source, first) in samples {
        let f = extract(path, lang, source);
        assert_eq!(f.grade, ParseGrade::Lexical, "{path}");
        assert_eq!(f.symbols.first().map(|s| s.name.as_str()), Some(first), "{path}");
    }
}

#[test]
fn nothing_panics_on_text_that_is_not_source() {
    for lang in [Lang::Python, Lang::Go, Lang::Ruby, Lang::Shell, Lang::Cpp] {
        for text in ["", "\u{0}\u{1}\u{2}", "}}}}((((", "\u{feff}", "def (", "'''", "<<<", "\\\n\\\n"] {
            let f = extract("x", lang, text);
            assert!(f.symbols.iter().all(|s| s.start <= s.end));
        }
    }
}

#[test]
fn extract_path_reads_a_file_and_ignores_other_extensions() {
    let root = std::env::temp_dir().join(format!("lxr-extract-fallback-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(root.join("src/lib.go"), "package p\n\nfunc F() {}\n").unwrap();
    std::fs::write(root.join("notes.txt"), "func G() {}\n").unwrap();

    let f = extract_path(&root, "src/lib.go").expect("a Go file is read");
    assert_eq!(f.path, "src/lib.go");
    assert_eq!(f.lang, Lang::Go);
    assert_eq!(f.grade, ParseGrade::Lexical);
    assert_eq!((f.symbols[0].name.as_str(), f.symbols[0].start, f.symbols[0].end), ("F", 3, 3));
    assert!(extract_path(&root, "notes.txt").is_none());
    assert!(extract_path(&root, "missing.go").is_none());
    std::fs::remove_dir_all(&root).unwrap();
}
