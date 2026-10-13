//! Cutting a text into facts, with no model: at sentence ends and line breaks.

/// A fact is at least this many characters, unless the whole text is shorter.
pub(crate) const MIN_FACT: usize = 20;
/// A fact is at most this many characters.
pub(crate) const MAX_FACT: usize = 400;

/// Cuts `text` into facts of 20 to 400 characters.
///
/// The cuts are at sentence ends and line breaks. Short neighbours are merged until a fact
/// reaches 20 characters, a sentence longer than 400 is cut at a space, and a text under 400
/// characters without a line break is one fact. Runs of white space become one space.
pub(crate) fn split_facts(text: &str) -> Vec<String> {
    let text = text.trim();
    if text.is_empty() {
        return Vec::new();
    }
    if !text.contains('\n') && text.chars().count() < MAX_FACT {
        return vec![collapse(text)];
    }
    let mut pieces: Vec<String> = Vec::new();
    for line in text.lines() {
        let line = strip_bullet(line.trim());
        if line.is_empty() {
            continue;
        }
        for sentence in sentences(line) {
            for part in cut_long(&sentence) {
                pieces.push(part);
            }
        }
    }
    merge(pieces)
}

/// One note from the whole text: edges trimmed, white space collapsed.
pub(crate) fn whole(text: &str) -> Option<String> {
    let text = collapse(text.trim());
    if text.is_empty() {
        None
    } else {
        Some(text)
    }
}

/// Merges short pieces into their neighbours so that facts are 20 characters or more
/// without growing past 400.
fn merge(pieces: Vec<String>) -> Vec<String> {
    let mut facts: Vec<String> = Vec::new();
    let mut current = String::new();
    for piece in pieces {
        if current.is_empty() {
            current = piece;
            continue;
        }
        let have = current.chars().count();
        if have < MIN_FACT && have + 1 + piece.chars().count() <= MAX_FACT {
            current.push(' ');
            current.push_str(&piece);
        } else {
            facts.push(std::mem::take(&mut current));
            current = piece;
        }
    }
    if !current.is_empty() {
        let tail = current.chars().count();
        match facts.last_mut() {
            Some(last) if tail < MIN_FACT && last.chars().count() + 1 + tail <= MAX_FACT => {
                last.push(' ');
                last.push_str(&current);
            }
            _ => facts.push(current),
        }
    }
    facts
}

/// The sentences of one line. A sentence ends at `.`, `!` or `?` that is followed by white
/// space and then something other than a lower-case letter, so `e.g. this` and `lib.rs` stay whole.
fn sentences(line: &str) -> Vec<String> {
    let chars: Vec<char> = line.chars().collect();
    let mut out = Vec::new();
    let mut start = 0;
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if matches!(c, '.' | '!' | '?') {
            let mut end = i + 1;
            while end < chars.len() && matches!(chars[end], '.' | '!' | '?' | '"' | '\'' | ')' | ']') {
                end += 1;
            }
            let at_end = end >= chars.len();
            let space_after = !at_end && chars[end].is_whitespace();
            if at_end || space_after {
                let mut next = end;
                while next < chars.len() && chars[next].is_whitespace() {
                    next += 1;
                }
                let continues = next < chars.len() && chars[next].is_lowercase();
                if at_end || !continues {
                    let piece: String = chars[start..end].iter().collect();
                    let piece = collapse(piece.trim());
                    if !piece.is_empty() {
                        out.push(piece);
                    }
                    start = next;
                    i = next;
                    continue;
                }
            }
            i = end;
            continue;
        }
        i += 1;
    }
    if start < chars.len() {
        let piece: String = chars[start..].iter().collect();
        let piece = collapse(piece.trim());
        if !piece.is_empty() {
            out.push(piece);
        }
    }
    out
}

/// Cuts a sentence longer than 400 characters at spaces so that no part exceeds 400.
fn cut_long(sentence: &str) -> Vec<String> {
    if sentence.chars().count() <= MAX_FACT {
        return vec![sentence.to_string()];
    }
    let chars: Vec<char> = sentence.chars().collect();
    let mut out = Vec::new();
    let mut start = 0;
    while chars.len() - start > MAX_FACT {
        let limit = start + MAX_FACT;
        let mut cut = limit;
        for at in (start + MIN_FACT..=limit).rev() {
            if chars[at - 1].is_whitespace() || (at < chars.len() && chars[at].is_whitespace()) {
                cut = at;
                break;
            }
        }
        let piece: String = chars[start..cut].iter().collect();
        let piece = piece.trim().to_string();
        if !piece.is_empty() {
            out.push(piece);
        }
        start = cut;
        while start < chars.len() && chars[start].is_whitespace() {
            start += 1;
        }
    }
    if start < chars.len() {
        out.push(chars[start..].iter().collect::<String>().trim().to_string());
    }
    out
}

/// Removes a list marker (`- `, `* `, `1. `, `2) `) from the start of a line.
fn strip_bullet(line: &str) -> &str {
    let bytes = line.as_bytes();
    if bytes.len() >= 2 && matches!(bytes[0], b'-' | b'*' | b'+') && bytes[1] == b' ' {
        return line[2..].trim_start();
    }
    let digits = bytes.iter().take_while(|b| b.is_ascii_digit()).count();
    if digits > 0 && digits <= 3 && bytes.len() > digits + 1 && matches!(bytes[digits], b'.' | b')') && bytes[digits + 1] == b' ' {
        return line[digits + 2..].trim_start();
    }
    line
}

/// Runs of white space become one space.
pub(crate) fn collapse(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut space = false;
    for c in text.chars() {
        if c.is_whitespace() {
            space = true;
        } else {
            if space && !out.is_empty() {
                out.push(' ');
            }
            space = false;
            out.push(c);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lens(facts: &[String]) -> Vec<usize> {
        facts.iter().map(|f| f.chars().count()).collect()
    }

    #[test]
    fn a_short_text_without_a_line_break_stays_whole() {
        let facts = split_facts("The loader reads notes. It is fast. It never panics.");
        assert_eq!(facts, vec!["The loader reads notes. It is fast. It never panics.".to_string()]);
    }

    #[test]
    fn empty_and_blank_texts_give_no_fact() {
        assert!(split_facts("").is_empty());
        assert!(split_facts("  \n \t ").is_empty());
        assert!(whole("   ").is_none());
    }

    #[test]
    fn line_breaks_cut_facts() {
        let facts = split_facts("The parser rejects empty files.\nThe writer flushes after every append.\n");
        assert_eq!(facts, vec!["The parser rejects empty files.", "The writer flushes after every append."]);
    }

    #[test]
    fn short_neighbours_are_merged_up_to_twenty_characters() {
        let facts = split_facts("Done.\nTests pass now.\nThe release build is ready for the benchmark run today.");
        assert!(facts.iter().all(|f| f.chars().count() >= MIN_FACT), "{facts:?}");
        assert_eq!(facts[0], "Done. Tests pass now.");
        assert_eq!(facts.len(), 2);
    }

    #[test]
    fn a_short_tail_joins_the_fact_before_it() {
        let facts = split_facts("The index keeps one entry per definition.\nOk.");
        assert_eq!(facts, vec!["The index keeps one entry per definition. Ok."]);
    }

    #[test]
    fn long_text_is_cut_at_sentence_ends_and_stays_within_the_cap() {
        let sentence = "The retain step cuts a text into facts at every sentence end.";
        let text = vec![sentence; 12].join(" ");
        assert!(text.chars().count() > MAX_FACT);
        let facts = split_facts(&text);
        assert_eq!(facts.len(), 12);
        assert!(facts.iter().all(|f| f == sentence));
    }

    #[test]
    fn one_sentence_over_the_cap_is_cut_at_spaces() {
        let text = vec!["word"; 200].join(" ");
        let facts = split_facts(&text);
        assert!(facts.len() >= 2);
        assert!(lens(&facts).iter().all(|&n| (MIN_FACT..=MAX_FACT).contains(&n)), "{:?}", lens(&facts));
        assert_eq!(facts.join(" "), text);
    }

    #[test]
    fn abbreviations_file_names_and_decimals_do_not_end_a_sentence() {
        let text = "See e.g. lib.rs for the 3.5 rule. The next sentence starts here.\nAnother line follows now.";
        let facts = split_facts(text);
        assert_eq!(facts, vec!["See e.g. lib.rs for the 3.5 rule.", "The next sentence starts here.", "Another line follows now."]);
    }

    #[test]
    fn list_markers_are_removed() {
        let facts = split_facts("- first item is about the cache\n* second item is about the log\n3. third item is about links");
        assert_eq!(facts, vec!["first item is about the cache", "second item is about the log", "third item is about links"]);
    }

    #[test]
    fn white_space_is_collapsed() {
        assert_eq!(whole("  a \t b\n\nc  ").as_deref(), Some("a b c"));
    }
}
