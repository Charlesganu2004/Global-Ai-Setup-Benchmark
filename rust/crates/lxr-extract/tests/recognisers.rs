//! What the extractors report for source with a trap in it, seen from the crate's public
//! function. The assertions are on spans, parents, names and signatures, the values an
//! extractor has to get right, not on invariants that the last pass would force anyway.

use lxr_core::{FileFacts, Lang, ParseGrade, SymKind};
use lxr_extract::extract;

/// One line per symbol: qualified name, kind, span and the index of the parent.
fn shape(f: &FileFacts) -> Vec<String> {
    f.symbols
        .iter()
        .map(|s| format!("{} {} {}-{} in {:?}", s.qual, s.kind.word(), s.start, s.end, s.parent))
        .collect()
}

fn sigs(f: &FileFacts) -> Vec<&str> {
    f.symbols.iter().map(|s| s.sig.as_str()).collect()
}

// ----- signatures carry no comments -----

#[test]
fn a_python_signature_leaves_out_the_comments_between_its_parameters() {
    let f = extract("a.py", Lang::Python, "def f(\n    a,  # the first\n    b,  # the second\n) -> int:\n    return a\n");
    assert_eq!(f.grade, ParseGrade::Ast);
    assert_eq!(sigs(&f), ["def f( a, b, ) -> int"]);
}

#[test]
fn a_javascript_signature_leaves_out_the_comments_between_its_parameters() {
    let f = extract("a.js", Lang::JavaScript, "function f(\n  a, // the first\n  b /* the second */\n) {\n  return a;\n}\n");
    assert_eq!(f.grade, ParseGrade::Ast);
    assert_eq!(sigs(&f), ["function f( a, b )"]);
}

#[test]
fn a_rust_signature_leaves_out_the_comments_between_its_parameters() {
    let f = extract("a.rs", Lang::Rust, "fn f(\n    a: u8, // the first\n    b: u8, // the second\n) -> u8 {\n    a\n}\n");
    assert_eq!(f.grade, ParseGrade::Ast);
    assert_eq!(sigs(&f), ["fn f( a: u8, b: u8, ) -> u8"]);
}

#[test]
fn a_java_signature_leaves_out_the_comments_between_its_parameters() {
    let f = extract("A.java", Lang::Java, "class A {\n    void f(\n        int a, // the first\n        int b /* the second */\n    ) {}\n}\n");
    assert_eq!(f.grade, ParseGrade::Lexical);
    assert_eq!(sigs(&f), ["class A", "void f( int a, int b )"]);
}

#[test]
fn a_string_that_looks_like_a_comment_stays_in_the_signature() {
    let f = extract("a.py", Lang::Python, "def f(url=\"http://x\", tag='#a'):\n    return url\n");
    assert_eq!(sigs(&f), ["def f(url=\"http://x\", tag='#a')"]);
}

// ----- a byte order mark -----

#[test]
fn a_byte_order_mark_does_not_hide_the_first_keyword_of_a_file() {
    let go = extract("a.go", Lang::Go, "\u{feff}func F() { g() }\nfunc H() {}\n");
    assert_eq!(shape(&go), ["F function 1-1 in None", "H function 2-2 in None"]);
    assert_eq!(go.calls.len(), 1);
    assert_eq!((go.calls[0].name.as_str(), go.calls[0].caller), ("g", Some(0)));

    let ruby = extract("a.rb", Lang::Ruby, "\u{feff}class Foo\n  def a\n  end\n  def b\n  end\nend\n");
    assert_eq!(shape(&ruby), ["Foo class 1-6 in None", "Foo.a method 2-3 in Some(0)", "Foo.b method 4-5 in Some(0)"]);

    let java = extract("A.java", Lang::Java, "\u{feff}public class A { }\n");
    assert_eq!(sigs(&java), ["public class A"]);
}

// ----- a C++ header named .h -----

#[test]
fn a_dot_h_file_that_uses_cplusplus_is_read_as_cplusplus() {
    let source = "class Foo : public Base { public: void bar() { baz(); } int qux() const; };\nnamespace n { void free_fn() {} }\n";
    let header = extract("a.h", Lang::C, source);
    let reference = extract("a.hpp", Lang::Cpp, source);
    assert_eq!(header.lang, Lang::C, "the language of the file is still the one its extension names");
    assert_eq!(header.symbols, reference.symbols);
    assert_eq!(header.calls, reference.calls);
    let found: Vec<(&str, SymKind)> = header.symbols.iter().map(|s| (s.qual.as_str(), s.kind)).collect();
    assert_eq!(found, [("Foo", SymKind::Class), ("Foo.bar", SymKind::Method), ("n.free_fn", SymKind::Function)]);
    assert_eq!(header.symbols[0].bases, reference.symbols[0].bases);
}

#[test]
fn a_dot_h_file_of_plain_c_is_read_as_it_always_was() {
    let source = "#ifndef X\n#define X\n#include <stdio.h>\nstruct point { int x; int y; };\nstatic int add(int a, int b) { return a + b; }\nint sub(int a, int b);\n#endif\n";
    let header = extract("x.h", Lang::C, source);
    let unit = extract("x.c", Lang::C, source);
    assert_eq!(header.symbols, unit.symbols);
    assert_eq!(header.calls, unit.calls);
    assert_eq!(header.imports, unit.imports);
    assert!(header.symbols.iter().any(|s| s.qual == "add" && s.kind == SymKind::Function));
}

#[test]
fn a_dot_c_file_is_never_read_as_cplusplus() {
    // `class` is an ordinary name in C.
    let f = extract("a.c", Lang::C, "int class = 1;\nint f(void) { return class; }\n");
    assert_eq!(shape(&f), ["f function 2-2 in None"]);
}

// ----- spans and parents -----

#[test]
fn unclosed_braces_nest_each_class_in_the_one_before_it() {
    let f = extract("A.java", Lang::Java, "class A {\nclass A {\nclass A {\n");
    assert_eq!(shape(&f), ["A class 1-3 in None", "A.A class 2-3 in Some(0)", "A.A.A class 3-3 in Some(1)"]);
}

#[test]
fn spans_and_parents_of_nested_definitions_in_the_languages_without_a_parser() {
    let go = extract("a.go", Lang::Go, "package p\n\ntype S struct{}\n\nfunc (s S) M() {\n\tg()\n}\n");
    assert_eq!(shape(&go), ["S struct 3-3 in None", "S.M method 5-7 in None"]);

    let kotlin = extract("a.kt", Lang::Kotlin, "class C {\n    fun m() {\n        g()\n    }\n}\n");
    assert_eq!(shape(&kotlin), ["C class 1-5 in None", "C.m method 2-4 in Some(0)"]);

    let csharp = extract("A.cs", Lang::CSharp, "namespace N\n{\n    class A\n    {\n        void M() { G(); }\n    }\n}\n");
    assert_eq!(shape(&csharp), ["N.A class 3-6 in None", "N.A.M method 5-5 in Some(0)"]);

    let php = extract("a.php", Lang::Php, "<?php\nclass A {\n    function m() { g(); }\n}\n");
    assert_eq!(shape(&php), ["A class 2-4 in None", "A.m method 3-3 in Some(0)"]);
}
