//! The code-aware tokeniser used for BM25 documents and queries.

/// Splits text into lowercase identifier tokens.
///
/// Identifiers are runs of `[a-zA-Z_][a-zA-Z0-9_]*`; everything else separates them.
/// Each identifier is emitted lowercased. When it splits into two or more parts, the
/// parts follow it: the non-empty pieces between underscores for snake_case, or the
/// words of a camelCase or PascalCase name (`getHTTPResponse` gives `get`, `http`,
/// `response`). There is no stop-word list and no stemming.
pub fn code_tokens(text: &str) -> Vec<String> {
    let bytes = text.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if !is_start(bytes[i]) {
            i += 1;
            continue;
        }
        let begin = i;
        while i < bytes.len() && is_continue(bytes[i]) {
            i += 1;
        }
        push_identifier(&bytes[begin..i], &mut out);
    }
    out
}

fn is_start(b: u8) -> bool {
    b.is_ascii_alphabetic() || b == b'_'
}

fn is_continue(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

/// Emits one identifier (ASCII only) and its parts.
fn push_identifier(token: &[u8], out: &mut Vec<String>) {
    let lower = lowered(token);
    let parts: Vec<String> = if token.contains(&b'_') {
        lower.split('_').filter(|p| !p.is_empty()).map(str::to_string).collect()
    } else {
        camel_parts(token)
    };
    if parts.len() >= 2 {
        out.push(lower);
        out.extend(parts);
    } else {
        out.push(lower);
    }
}

fn lowered(token: &[u8]) -> String {
    token.iter().map(|b| b.to_ascii_lowercase() as char).collect()
}

/// The lowercased matches of `[A-Z]+(?=[A-Z][a-z])|[A-Z]?[a-z]+|[A-Z]+|[0-9]+`, left
/// to right. The regex crate has no look-ahead, so the first alternative is done by hand.
fn camel_parts(token: &[u8]) -> Vec<String> {
    let mut parts = Vec::new();
    let mut i = 0;
    while i < token.len() {
        let upper = run(token, i, u8::is_ascii_uppercase);
        let end = if upper >= 2 && token.get(i + upper).is_some_and(u8::is_ascii_lowercase) {
            // [A-Z]+ backs off by one so that the look-ahead sees an upper then a lower case letter.
            Some(i + upper - 1)
        } else if upper <= 1 && run(token, i + upper, u8::is_ascii_lowercase) > 0 {
            // [A-Z]?[a-z]+
            Some(i + upper + run(token, i + upper, u8::is_ascii_lowercase))
        } else if upper >= 1 {
            // [A-Z]+
            Some(i + upper)
        } else if token[i].is_ascii_digit() {
            // [0-9]+
            Some(i + run(token, i, u8::is_ascii_digit))
        } else {
            None
        };
        match end {
            Some(end) => {
                parts.push(lowered(&token[i..end]));
                i = end;
            }
            None => i += 1,
        }
    }
    parts
}

/// How many bytes from `from` on satisfy `keep`.
fn run(token: &[u8], from: usize, keep: fn(&u8) -> bool) -> usize {
    token.get(from..).map_or(0, |rest| rest.iter().take_while(|b| keep(b)).count())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(text: &str) -> Vec<String> {
        code_tokens(text)
    }

    fn v(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn empty_and_separator_only_text_give_nothing() {
        assert!(t("").is_empty());
        assert!(t("  123 -- ()[] 42").is_empty());
        assert!(t("\u{e9}\u{4e2d}").is_empty());
    }

    #[test]
    fn plain_word_is_lowercased_once() {
        assert_eq!(t("Hello"), v(&["hello"]));
        assert_eq!(t("simple"), v(&["simple"]));
    }

    #[test]
    fn snake_case_emits_whole_then_parts() {
        assert_eq!(t("my_func"), v(&["my_func", "my", "func"]));
        assert_eq!(t("__init__"), v(&["__init__"]));
        assert_eq!(t("_"), v(&["_"]));
        assert_eq!(t("a_1"), v(&["a_1", "a", "1"]));
        assert_eq!(t("MAX_VALUE"), v(&["max_value", "max", "value"]));
    }

    #[test]
    fn camel_case_follows_the_pattern_in_the_spec() {
        assert_eq!(t("HandlerStack"), v(&["handlerstack", "handler", "stack"]));
        assert_eq!(t("getHTTPResponse"), v(&["gethttpresponse", "get", "http", "response"]));
        assert_eq!(t("XMLParser"), v(&["xmlparser", "xml", "parser"]));
        assert_eq!(t("ABC"), v(&["abc"]));
        assert_eq!(t("x86"), v(&["x86", "x", "86"]));
        assert_eq!(t("x1F"), v(&["x1f", "x", "1", "f"]));
        assert_eq!(t("parseJSON2Value"), v(&["parsejson2value", "parse", "json", "2", "value"]));
    }

    #[test]
    fn text_is_scanned_for_identifiers() {
        assert_eq!(
            t("let fooBar = baz_qux(1);"),
            v(&["let", "foobar", "foo", "bar", "baz_qux", "baz", "qux"])
        );
    }

    #[test]
    fn digits_cannot_start_an_identifier() {
        assert_eq!(t("2fa"), v(&["fa"]));
        assert_eq!(t("0x1F"), v(&["x1f", "x", "1", "f"]));
    }

    #[test]
    fn non_ascii_letters_separate_identifiers() {
        assert_eq!(t("abc\u{e9}def"), v(&["abc", "def"]));
    }
}
