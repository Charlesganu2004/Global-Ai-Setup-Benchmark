//! Scenarios from a second, read-only review of the extractor, run through the public
//! interface. A test that pins the present behaviour on purpose says which sentence of the
//! contract in `lxr-core/src/model.rs` it stretches and which crate depends on it.

use lxr_core::{hash64, FileFacts, Lang, ParseGrade};
use lxr_extract::{extract, extract_path};
use std::time::{Duration, Instant};

const BOUND: Duration = Duration::from_secs(5);

fn timed<T>(work: impl FnOnce() -> T) -> (T, Duration) {
    let started = Instant::now();
    let result = work();
    (result, started.elapsed())
}

fn names(facts: &FileFacts) -> Vec<&str> {
    facts.symbols.iter().map(|s| s.name.as_str()).collect()
}

fn called(facts: &FileFacts) -> Vec<&str> {
    facts.calls.iter().map(|c| c.name.as_str()).collect()
}

/// The index of the symbol with this qualified name.
fn index_of(facts: &FileFacts, qual: &str) -> u32 {
    facts.symbols.iter().position(|s| s.qual == qual).unwrap_or_else(|| panic!("no symbol {qual} in {:?}", names(facts))) as u32
}

/// The qualified name of the function a call sits in, `-` for none.
fn caller_of(facts: &FileFacts, name: &str) -> String {
    let call = facts.calls.iter().find(|c| c.name == name).unwrap_or_else(|| panic!("no call {name} in {:?}", called(facts)));
    call.caller.map_or("-".to_string(), |k| facts.symbols[k as usize].qual.clone())
}

#[test]
fn a_chain_of_fifty_thousand_method_calls_keeps_one_short_receiver_per_call() {
    let text = format!("x{}\n", ".m()".repeat(50_000));
    let (facts, took) = timed(|| extract("chain.py", Lang::Python, &text));
    assert_eq!(facts.grade, ParseGrade::Ast);
    assert_eq!(facts.calls.len(), 50_000);
    let kept: usize = facts.calls.iter().map(|c| c.recv.len()).sum();
    assert!(kept <= 50_000 * 60, "{kept} bytes of receiver text kept");
    assert!(took < BOUND, "the chain took {took:?}");
}

#[test]
fn a_run_of_hash_marks_in_swift_is_read_once() {
    let text = format!("{}\nfunc after() {{}}\n", "#".repeat(150_000));
    let (facts, took) = timed(|| extract("hashes.swift", Lang::Swift, &text));
    assert!(names(&facts).contains(&"after"), "{:?}", names(&facts));
    assert!(took < BOUND, "a run of 150,000 hash marks took {took:?}");
}

#[test]
fn a_raw_string_after_a_run_of_hash_marks_in_swift_is_still_a_string() {
    let text = "####\nlet s = ##\"func hidden() {} \"hi\"# still inside\"##\nfunc shown() {}\n";
    let facts = extract("raw.swift", Lang::Swift, text);
    assert_eq!(names(&facts), ["shown"]);
}

#[test]
fn heredocs_without_a_closing_line_are_not_searched_for_again_by_each() {
    let same = format!("build() {{ :; }}\n{}", "<<EOF\n".repeat(40_000));
    let (facts, took) = timed(|| extract("same.sh", Lang::Shell, &same));
    assert_eq!(names(&facts), ["build"]);
    assert!(took < BOUND, "40,000 unclosed heredocs with one delimiter took {took:?}");

    let distinct: String = std::iter::once("build() { :; }\n".to_string()).chain((0..40_000).map(|i| format!("<<D{i}\n"))).collect();
    let (facts, took) = timed(|| extract("distinct.sh", Lang::Shell, &distinct));
    assert_eq!(names(&facts), ["build"]);
    assert!(took < BOUND, "40,000 unclosed heredocs with distinct delimiters took {took:?}");
}

#[test]
fn candidates_for_a_heredoc_on_one_long_line_are_not_each_read_to_its_end() {
    let text = format!("build() {{ :; }}\ncat {}\n", "<<E ".repeat(100_000));
    let (facts, took) = timed(|| extract("line.sh", Lang::Shell, &text));
    assert_eq!(names(&facts), ["build"]);
    assert!(took < BOUND, "100,000 unclosed heredocs on one line took {took:?}");
}

#[test]
fn many_heredocs_on_one_command_line_are_read_in_one_pass() {
    let text = format!("cat {}\n{}real() {{ :; }}\n", "<<A ".repeat(30_000), "A\n".repeat(30_000));
    let (facts, took) = timed(|| extract("many.sh", Lang::Shell, &text));
    assert_eq!(names(&facts), ["real"]);
    assert!(took < BOUND, "30,000 heredocs on one line took {took:?}");
}

#[test]
fn a_closed_heredoc_still_hides_its_body_in_shell() {
    let facts = extract("one.sh", Lang::Shell, "cat <<EOF\nghost() { :; }\nEOF\nreal() { :; }\n");
    assert_eq!(names(&facts), ["real"]);
}

#[test]
fn every_heredoc_on_a_command_line_hides_its_own_body() {
    let two = "cat <<A <<B\none\nA\nghost() { :; }\nB\nreal() { :; }\n";
    assert_eq!(names(&extract("two.sh", Lang::Shell, two)), ["real"]);

    let three = "cat <<A <<B <<C\na\nA\nb() { :; }\nB\nc() { :; }\nC\nreal() { :; }\n";
    assert_eq!(names(&extract("three.sh", Lang::Shell, three)), ["real"]);

    let ruby = "puts(<<A, <<B)\none\nA\ndef ghost\nend\nB\ndef real\nend\n";
    assert_eq!(names(&extract("two.rb", Lang::Ruby, ruby)), ["real"]);
}

#[test]
fn bytes_and_hash_are_those_of_the_file_on_disk_when_it_is_not_utf8() {
    let root = std::env::temp_dir().join(format!("lxr-extract-review-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let files: [(&str, &[u8]); 3] = [("a.js", b"// \xff\n"), ("b.js", b"// \xfe\n"), ("c.js", b"// x\nfunction f() {}\n")];
    for (name, content) in files {
        std::fs::write(root.join(name), content).unwrap();
    }
    let facts: Vec<FileFacts> = files.iter().map(|(name, _)| extract_path(&root, name).expect("a known language")).collect();
    std::fs::remove_dir_all(&root).unwrap();

    for (fact, (name, content)) in facts.iter().zip(files) {
        assert_eq!(fact.bytes, content.len() as u64, "{name}");
        assert_eq!(fact.hash, hash64(content), "{name}");
    }
    assert_ne!(facts[0].hash, facts[1].hash, "two different bad bytes read the same");
}

#[test]
fn symbols_that_start_on_one_line_stay_in_source_order() {
    let rust = extract("a.rs", Lang::Rust, "fn a() {} fn b() {\n}\n");
    assert_eq!(names(&rust), ["a", "b"]);

    let js = extract("a.js", Lang::JavaScript, "function a() {} function b() {\n}\n");
    assert_eq!(names(&js), ["a", "b"]);

    let impls = extract("b.rs", Lang::Rust, "impl X { fn a() {} fn b() {\n} }\n");
    assert_eq!(names(&impls), ["X", "a", "b"]);
    assert_eq!((impls.symbols[1].parent, impls.symbols[2].parent), (Some(0), Some(0)));

    let java = extract("A.java", Lang::Java, "class A { void m() {} void n() {\n} }\n");
    assert_eq!(java.grade, ParseGrade::Lexical);
    assert_eq!(names(&java), ["A", "m", "n"]);
    assert_eq!((java.symbols[1].parent, java.symbols[2].parent), (Some(0), Some(0)));

    let class = extract("c.js", Lang::JavaScript, "class A { m() {} n() {\n} }\n");
    assert_eq!(names(&class), ["A", "m", "n"]);
}

#[test]
fn calls_on_one_line_come_in_source_order() {
    let python = extract("a.py", Lang::Python, "x.first().second()\ny = a() if b() else c()\nx.first(a()).second(b())\n");
    assert_eq!(python.grade, ParseGrade::Ast);
    assert_eq!(called(&python), ["first", "second", "a", "b", "c", "first", "a", "second", "b"]);

    let js = extract("a.js", Lang::JavaScript, "x.first().second();\nx.first(a()).second(b());\n");
    assert_eq!(js.grade, ParseGrade::Ast);
    assert_eq!(called(&js), ["first", "second", "first", "a", "second", "b"]);

    let rust = extract("a.rs", Lang::Rust, "fn f() {\n    x.first().second();\n    x.first(a()).second(b());\n    foo(a).bar(baz());\n}\n");
    assert_eq!(rust.grade, ParseGrade::Ast);
    assert_eq!(called(&rust), ["first", "second", "first", "a", "second", "b", "foo", "bar", "baz"]);
    // The receiver goes with its call, whatever the order of the walk.
    assert_eq!(rust.calls[0].recv, "x");
    assert_eq!(rust.calls[1].recv, "x.first()");
}

/// The contract says a call has no caller "in a class body". A class inside a function runs when
/// that function runs, and the graph resolves the names in it from the function's scope, so the
/// function is the caller; only a class outside every function gives `None`.
#[test]
fn a_class_body_inside_a_function_is_called_by_that_function() {
    let python = extract("a.py", Lang::Python, "def outer():\n    class C:\n        value = build()\nclass D:\n    other = make()\n");
    assert_eq!(python.grade, ParseGrade::Ast);
    assert_eq!(caller_of(&python, "build"), "outer");
    assert_eq!(caller_of(&python, "make"), "-");

    let js = extract("a.js", Lang::JavaScript, "function outer() {\n  class C {\n    static value = build();\n  }\n}\nclass D {\n  static v = make();\n}\n");
    assert_eq!(js.grade, ParseGrade::Ast);
    assert_eq!(caller_of(&js, "build"), "outer");
    assert_eq!(caller_of(&js, "make"), "-");
}

#[test]
fn items_inside_a_constant_initialiser_sit_in_the_constant() {
    let text = "\
const X: () = {
    fn inner() {}
};
static S: u8 = {
    fn make() -> u8 { 1 }
    make()
};
fn outer() {
    const K: u8 = helper();
}
struct T;
impl T {
    const Z: () = {
        fn deep() {}
    };
}
trait Tr {
    const D: () = {
        fn dflt() {}
    };
}
";
    let facts = extract("c.rs", Lang::Rust, text);
    assert_eq!(facts.grade, ParseGrade::Ast);
    for (inner, container) in [("X.inner", "X"), ("S.make", "S"), ("T.Z.deep", "T.Z"), ("Tr.D.dflt", "Tr.D")] {
        let at = index_of(&facts, inner) as usize;
        assert_eq!(facts.symbols[at].parent, Some(index_of(&facts, container)), "{inner}");
    }
    // A constant is not a function: calls in its initialiser belong to the function around it.
    assert_eq!(caller_of(&facts, "make"), "-");
    assert_eq!(caller_of(&facts, "helper"), "outer");
}

/// The contract says `alias` is "equal to `name` (or to the module's last part) when there is no
/// `as`". For `import a.b` the extractor records the whole dotted name, and the graph's
/// `bind_python` reads it that way: it binds both `a.b` and the package `a` from it, so that
/// `a.b.f()` and `a.g()` resolve. An alias of `a` or `b` would drop the first of those.
#[test]
fn a_dotted_python_import_records_the_whole_module_as_its_alias() {
    let facts = extract("a.py", Lang::Python, "import os.path\nimport os.path as p\nimport a.b.c\nfrom os import path\n");
    let imports: Vec<(&str, &str, &str)> = facts.imports.iter().map(|i| (i.module.as_str(), i.name.as_str(), i.alias.as_str())).collect();
    assert_eq!(imports, [("os.path", "", "os.path"), ("os.path", "", "p"), ("a.b.c", "", "a.b.c"), ("os", "path", "path")]);
}

/// The contract says a receiver is "as written". For a Rust path call the extractor records the
/// path before the last `::` as plain identifiers, without generic arguments and without a
/// qualified self. The graph splits a receiver at `::` and looks each part up as a name
/// (`class_of_receiver`, `module_of_receiver`), which `Vec::<u8>` or `<Widget as Trait>` would defeat.
#[test]
fn rust_path_calls_record_a_plain_path_as_the_receiver() {
    let facts = extract("a.rs", Lang::Rust, "fn f() {\n    Vec::<u8>::new();\n    <Widget as Trait>::run();\n    a::b::c();\n}\n");
    assert_eq!(facts.grade, ParseGrade::Ast);
    let calls: Vec<(&str, &str)> = facts.calls.iter().map(|c| (c.recv.as_str(), c.name.as_str())).collect();
    assert_eq!(calls, [("Vec", "new"), ("Trait", "run"), ("a::b", "c")]);
}

/// The contract says `bases` are "as written". A heritage clause of JavaScript or TypeScript is
/// recorded by the name of the type, without its type arguments; Rust and Python keep theirs.
/// The graph removes type arguments from every base (`clean_base`), so both forms resolve.
#[test]
fn ecma_bases_are_names_without_type_arguments() {
    let facts = extract("a.ts", Lang::TypeScript, "class C extends Base<string> implements Other<string> {}\ninterface I extends Base<number> {}\n");
    assert_eq!(facts.grade, ParseGrade::Ast);
    assert_eq!(facts.symbols[0].bases, ["Base", "Other"]);
    assert_eq!(facts.symbols[1].bases, ["Base"]);
}
