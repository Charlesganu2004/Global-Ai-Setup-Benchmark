//! The lines that answers are made of: definition rows, caller rows, line lists.

use lxr_graph::{Graph, Link, NodeKind};

/// A row is cut at this many characters.
pub const ROW_LIMIT: usize = 300;

/// At most this many call lines are listed for one caller, then `+K`.
const LINES_SHOWN: usize = 6;

/// Cuts a row to `ROW_LIMIT` characters.
pub fn cut_row(row: String) -> String {
    cut(row, ROW_LIMIT)
}

/// Cuts text to at most `max` characters.
pub fn cut(text: String, max: usize) -> String {
    match text.char_indices().nth(max) {
        Some((at, _)) => text[..at].to_string(),
        None => text,
    }
}

/// The signature of a function from its opening parenthesis on: for
/// `def parse(self, text: str) -> Config` that is `(self, text: str) -> Config`.
/// A parenthesis inside generic brackets (`fn run<F: Fn()>(f: F)`) does not count.
/// The whole text comes back when there is no such parenthesis.
pub fn call_tail(sig: &str) -> &str {
    let mut depth = 0i32;
    let mut previous = ' ';
    for (at, c) in sig.char_indices() {
        match c {
            '<' | '[' => depth += 1,
            '>' if previous != '-' => depth = (depth - 1).max(0),
            ']' => depth = (depth - 1).max(0),
            '(' if depth == 0 => return &sig[at..],
            _ => {}
        }
        previous = c;
    }
    sig
}

/// True when a declaration, seen without its kind, is that of a function or method:
/// it has a parenthesis, no assignment before it and no word that starts a class-like or
/// constant declaration.
pub fn looks_callable(sig: &str) -> bool {
    const OTHER: &[&str] = &[
        "class", "struct", "enum", "trait", "interface", "impl", "type", "const", "static", "let", "var", "mod", "namespace",
        "protocol", "object", "record", "union", "typedef", "module",
    ];
    let head = match sig.find('(') {
        Some(at) => &sig[..at],
        None => return false,
    };
    !head.contains('=') && !head.split(|c: char| !c.is_alphanumeric() && c != '_').any(|word| OTHER.contains(&word))
}

/// The lines of the calls, `@12,40`, at most six, then `,+K` for the rest.
pub fn at_lines(lines: &[u32]) -> String {
    if lines.is_empty() {
        return String::new();
    }
    let shown: Vec<String> = lines.iter().take(LINES_SHOWN).map(|l| l.to_string()).collect();
    let mut text = format!("@{}", shown.join(","));
    if lines.len() > LINES_SHOWN {
        text.push_str(&format!(",+{}", lines.len() - LINES_SHOWN));
    }
    text
}

/// `NAME PATH:FIRST-LAST SIGNATURE`: the definition row of a node. A file node has
/// no signature.
pub fn def_row(graph: &Graph, node: u32) -> String {
    let n = &graph.nodes[node as usize];
    let mut row = format!("{} {}:{}-{}", n.qual, graph.path_of(node), n.start, n.end);
    if let NodeKind::Symbol(kind) = n.kind {
        let sig = if kind.is_callable() { call_tail(&n.sig) } else { n.sig.as_str() };
        if !sig.is_empty() {
            row.push(' ');
            row.push_str(sig);
        }
    }
    cut_row(row)
}

/// `NAME PATH:FIRST-LAST`: a node without its signature.
pub fn span_row(graph: &Graph, node: u32) -> String {
    let n = &graph.nodes[node as usize];
    format!("{} {}:{}-{}", n.qual, graph.path_of(node), n.start, n.end)
}

/// `NAME PATH:FIRST`: the short reference used in paths and impact lists.
pub fn ref_row(graph: &Graph, node: u32) -> String {
    let n = &graph.nodes[node as usize];
    format!("{} {}:{}", n.qual, graph.path_of(node), n.start)
}

/// `CALLER PATH:FIRST-LAST @L1,L2`, or `<module> PATH @L` for a call at module level.
pub fn caller_row(graph: &Graph, link: &Link) -> String {
    let n = &graph.nodes[link.node as usize];
    let mut row = match n.kind {
        NodeKind::File => format!("<module> {}", graph.path_of(link.node)),
        NodeKind::Symbol(_) => span_row(graph, link.node),
    };
    let lines = at_lines(&link.lines);
    if !lines.is_empty() {
        row.push(' ');
        row.push_str(&lines);
    }
    row
}

/// The names in a list, at most `max`, with `, +K` for the rest.
pub fn name_list(names: &[String], max: usize) -> String {
    let mut text = names.iter().take(max).cloned().collect::<Vec<_>>().join(", ");
    if names.len() > max {
        text.push_str(&format!(", +{}", names.len() - max));
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn call_tail_starts_at_the_parameter_list() {
        assert_eq!(call_tail("def parse(self, text: str) -> Config"), "(self, text: str) -> Config");
        assert_eq!(call_tail("async def run(x)"), "(x)");
        assert_eq!(call_tail("pub fn run<F: Fn() -> u8>(f: F) -> u8"), "(f: F) -> u8");
        assert_eq!(call_tail("fn f<T: Fn(A) -> B, F: Fn(C)>(x: T)"), "(x: T)");
        assert_eq!(call_tail("class Foo"), "class Foo");
    }

    #[test]
    fn callable_and_class_like_declarations_are_told_apart() {
        assert!(looks_callable("def parse(self)"));
        assert!(looks_callable("pub fn run(x: u8)"));
        assert!(looks_callable("render(props)"));
        assert!(!looks_callable("class Foo(Base)"));
        assert!(!looks_callable("struct Foo"));
        assert!(!looks_callable("const X: u8"));
        assert!(!looks_callable("STARTUP = main([\"a\"])"));
    }

    #[test]
    fn line_lists_stop_at_six() {
        assert_eq!(at_lines(&[]), "");
        assert_eq!(at_lines(&[3]), "@3");
        assert_eq!(at_lines(&[1, 2, 3, 4, 5, 6]), "@1,2,3,4,5,6");
        assert_eq!(at_lines(&[1, 2, 3, 4, 5, 6, 7, 8]), "@1,2,3,4,5,6,+2");
    }

    #[test]
    fn rows_are_cut_in_characters() {
        let long: String = "é".repeat(400);
        assert_eq!(cut_row(long).chars().count(), 300);
        assert_eq!(cut("abc".to_string(), 5), "abc");
    }

    #[test]
    fn name_lists_count_the_rest() {
        let names: Vec<String> = ["a", "b", "c", "d"].iter().map(|s| s.to_string()).collect();
        assert_eq!(name_list(&names, 3), "a, b, c, +1");
        assert_eq!(name_list(&names, 12), "a, b, c, d");
    }
}
