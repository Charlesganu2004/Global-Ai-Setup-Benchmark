//! `notes.jsonl`: one JSON object per line. A change to a note appends its new full state,
//! a forget appends a tombstone, and the last line for an id wins. A line that does not
//! parse is skipped, so a write cut short by a crash costs at most that one line.

use crate::Note;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io;

/// The line that says a note was forgotten.
#[derive(Serialize, Deserialize)]
struct Tombstone {
    forgotten: String,
    #[serde(default)]
    at: i64,
}

/// What one line of the log says.
pub(crate) enum Entry {
    /// The full state of a note.
    Note(Note),
    /// The id of a note that was forgotten.
    Forget(String),
}

/// The id of the note with sequence number `seq`: `n000042`.
pub(crate) fn id_for(seq: u64) -> String {
    format!("n{seq:06}")
}

/// The sequence number inside an id made by [`id_for`].
pub(crate) fn id_seq(id: &str) -> Option<u64> {
    let digits = id.strip_prefix('n')?;
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    digits.parse().ok()
}

fn trim(mut line: &[u8]) -> &[u8] {
    while let [first, rest @ ..] = line {
        if first.is_ascii_whitespace() {
            line = rest;
        } else {
            break;
        }
    }
    while let [rest @ .., last] = line {
        if last.is_ascii_whitespace() {
            line = rest;
        } else {
            break;
        }
    }
    line
}

/// Reads one line. `None` for a blank line or one that is not a note or a tombstone.
pub(crate) fn parse_line(line: &[u8]) -> Option<Entry> {
    let line = trim(line);
    if line.is_empty() {
        return None;
    }
    if let Ok(note) = serde_json::from_slice::<Note>(line) {
        if !note.id.is_empty() {
            return Some(Entry::Note(note));
        }
    }
    serde_json::from_slice::<Tombstone>(line).ok().filter(|t| !t.forgotten.is_empty()).map(|t| Entry::Forget(t.forgotten))
}

/// The log line for a note, with its line break.
pub(crate) fn note_line(note: &Note) -> io::Result<String> {
    let mut line = serde_json::to_string(note).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    line.push('\n');
    Ok(line)
}

/// The log line that forgets a note, with its line break.
pub(crate) fn forget_line(id: &str, at: i64) -> io::Result<String> {
    let mut line =
        serde_json::to_string(&Tombstone { forgotten: id.to_string(), at }).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    line.push('\n');
    Ok(line)
}

/// The state a log describes.
pub(crate) struct Replay {
    /// One entry per id in order of first appearance; `None` once forgotten.
    pub slots: Vec<Option<Note>>,
    /// Slot of every id the log mentions, forgotten ones included.
    pub ids: HashMap<String, u32>,
    /// Highest sequence number in any id the log mentions.
    pub max_seq: u64,
}

/// Plays a whole log: the last line for an id wins.
pub(crate) fn replay(bytes: &[u8]) -> Replay {
    let mut out = Replay { slots: Vec::new(), ids: HashMap::new(), max_seq: 0 };
    for line in bytes.split(|&b| b == b'\n') {
        match parse_line(line) {
            Some(Entry::Note(note)) => {
                out.max_seq = out.max_seq.max(id_seq(&note.id).unwrap_or(0));
                match out.ids.get(&note.id) {
                    Some(&slot) => out.slots[slot as usize] = Some(note),
                    None => {
                        out.ids.insert(note.id.clone(), out.slots.len() as u32);
                        out.slots.push(Some(note));
                    }
                }
            }
            Some(Entry::Forget(id)) => {
                out.max_seq = out.max_seq.max(id_seq(&id).unwrap_or(0));
                if let Some(&slot) = out.ids.get(&id) {
                    out.slots[slot as usize] = None;
                }
            }
            None => {}
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Kind;

    fn note(id: &str, text: &str) -> Note {
        Note { id: id.to_string(), kind: Kind::World, text: text.to_string(), proof_count: 1, ..Note::default() }
    }

    fn log(lines: &[String]) -> Vec<u8> {
        lines.concat().into_bytes()
    }

    #[test]
    fn ids_carry_their_sequence_number() {
        assert_eq!(id_for(42), "n000042");
        assert_eq!(id_seq("n000042"), Some(42));
        assert_eq!(id_seq(&id_for(1_234_567)), Some(1_234_567));
        assert_eq!(id_seq("x1"), None);
        assert_eq!(id_seq("n"), None);
        assert_eq!(id_seq("n12a"), None);
    }

    #[test]
    fn a_note_line_parses_back() {
        let mut n = note("n000001", "text with \"quotes\" and a\nnewline");
        n.tags = vec!["a".into()];
        n.occurred_start = Some(5);
        let line = note_line(&n).unwrap();
        assert!(line.ends_with('\n') && line.matches('\n').count() == 1);
        match parse_line(line.as_bytes()) {
            Some(Entry::Note(back)) => assert_eq!(back, n),
            _ => panic!("not a note"),
        }
    }

    #[test]
    fn the_last_line_for_an_id_wins_and_slots_keep_first_appearance_order() {
        let bytes = log(&[
            note_line(&note("n000001", "one")).unwrap(),
            note_line(&note("n000002", "two")).unwrap(),
            note_line(&note("n000001", "one again")).unwrap(),
        ]);
        let r = replay(&bytes);
        assert_eq!(r.slots.len(), 2);
        assert_eq!(r.slots[0].as_ref().unwrap().text, "one again");
        assert_eq!(r.slots[1].as_ref().unwrap().text, "two");
        assert_eq!(r.max_seq, 2);
    }

    #[test]
    fn a_tombstone_removes_a_note_and_a_later_state_brings_it_back() {
        let mut lines = vec![note_line(&note("n000001", "one")).unwrap(), forget_line("n000001", 9).unwrap()];
        let r = replay(&log(&lines));
        assert!(r.slots[0].is_none());
        assert_eq!(r.ids["n000001"], 0);
        lines.push(note_line(&note("n000001", "back")).unwrap());
        assert_eq!(replay(&log(&lines)).slots[0].as_ref().unwrap().text, "back");
    }

    #[test]
    fn a_tombstone_for_an_unknown_id_still_counts_for_the_sequence() {
        let r = replay(&log(&[forget_line("n000077", 0).unwrap()]));
        assert!(r.slots.is_empty());
        assert_eq!(r.max_seq, 77);
    }

    #[test]
    fn damaged_blank_and_foreign_lines_are_skipped() {
        let mut bytes = log(&[note_line(&note("n000001", "one")).unwrap(), "\n".to_string(), "not json\n".to_string()]);
        bytes.extend_from_slice(b"{\"hello\":1}\r\n");
        bytes.extend_from_slice(note_line(&note("n000002", "two")).unwrap().as_bytes());
        // The last line is cut in the middle, as a crash would leave it.
        let cut = note_line(&note("n000003", "three")).unwrap();
        bytes.extend_from_slice(&cut.as_bytes()[..cut.len() / 2]);
        let r = replay(&bytes);
        let texts: Vec<&str> = r.slots.iter().flatten().map(|n| n.text.as_str()).collect();
        assert_eq!(texts, vec!["one", "two"]);
    }

    #[test]
    fn a_line_with_a_carriage_return_and_spaces_parses() {
        let line = format!("  {}\r\n", note_line(&note("n000001", "x")).unwrap().trim_end());
        assert!(matches!(parse_line(line.as_bytes()), Some(Entry::Note(_))));
    }

    #[test]
    fn a_line_may_leave_out_the_optional_fields() {
        let r = replay(br#"{"id":"n000009","kind":"Directive","text":"always run the tests"}"#);
        let n = r.slots[0].as_ref().unwrap();
        assert_eq!(n.kind, Kind::Directive);
        assert_eq!(n.proof_count, 1);
        assert!(n.tags.is_empty() && n.anchors.is_empty());
    }
}
