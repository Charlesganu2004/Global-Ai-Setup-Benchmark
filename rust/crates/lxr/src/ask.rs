//! `pgr ask KIND:WHAT...`: several questions in one process, each answer under its own
//! `== kind:what` line, followed by the notes that go with them when there are any.

use crate::cli::{FindArgs, RecallArgs, DEFAULT_DEPTH, DEFAULT_FIND_K};
use crate::ctx::{Ctx, Engine, Needs};
use crate::fail::{Fail, Res};
use crate::{findcmd, graphcmd, memcmd};
use lxr_rag::Mode;

/// Most notes the closing `== memory` section holds, and its size in tokens.
const MEMORY_NOTES: usize = 4;
const MEMORY_TOKENS: usize = 200;

/// What a question asks for.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Def,
    Callers,
    About,
    Affected,
    Explain,
    Find,
    Recall,
    Path,
}

impl Kind {
    fn from_word(word: &str) -> Option<Kind> {
        Some(match word.trim().to_ascii_lowercase().as_str() {
            "def" => Kind::Def,
            "callers" => Kind::Callers,
            "about" => Kind::About,
            "affected" => Kind::Affected,
            "explain" => Kind::Explain,
            "find" => Kind::Find,
            "recall" => Kind::Recall,
            "path" => Kind::Path,
            _ => return None,
        })
    }

    /// The engine that answers this kind of question.
    fn engine(self) -> Engine {
        match self {
            Kind::Find => Engine::Search,
            Kind::Recall => Engine::Memory,
            _ => Engine::Graph,
        }
    }
}

/// One `KIND:WHAT` argument.
#[derive(Clone, Debug)]
pub struct Question {
    pub kind: Kind,
    /// The text after the first colon.
    pub what: String,
    /// The argument as it was typed, used for the `== ` line.
    pub raw: String,
}

impl Question {
    /// The words of the question as plain text, for looking up notes.
    fn words(&self) -> String {
        self.what.replace('>', " ")
    }
}

/// Reads the `KIND:WHAT` arguments. A kind that is not known, or a missing colon or
/// text, is a usage error.
pub fn parse(args: &[String]) -> Res<Vec<Question>> {
    let mut questions = Vec::with_capacity(args.len());
    for raw in args {
        let (word, what) = raw
            .split_once(':')
            .ok_or_else(|| Fail::usage(format!("ask needs KIND:WHAT, got '{raw}'")))?;
        let kind = Kind::from_word(word).ok_or_else(|| {
            Fail::usage(format!("unknown kind '{}' (use def, callers, about, affected, explain, find, recall, path)", word.trim()))
        })?;
        let what = what.trim();
        if what.is_empty() {
            return Err(Fail::usage(format!("ask needs some text after the colon in '{raw}'")));
        }
        if kind == Kind::Path && what.split_once('>').map_or(true, |(a, b)| a.trim().is_empty() || b.trim().is_empty()) {
            return Err(Fail::usage(format!("a path question is path:A>B, got '{raw}'")));
        }
        questions.push(Question { kind, what: what.to_string(), raw: raw.trim().to_string() });
    }
    Ok(questions)
}

/// Answers the questions in order.
pub fn run(ctx: &mut Ctx, questions: &[Question]) -> Res<String> {
    for question in questions {
        ctx.engines.require(question.kind.engine())?;
    }
    let needs = Needs {
        graph: questions.iter().any(|q| q.kind.engine() == Engine::Graph),
        search: questions.iter().any(|q| q.kind == Kind::Find),
        embed: false,
        // The questions of a find kind are searches by words and meaning.
        vectors: questions.iter().any(|q| q.kind == Kind::Find),
    };
    if needs.graph || needs.search {
        ctx.need(needs)?;
    }
    let mut out = String::new();
    for question in questions {
        ln!(out, "== {}", question.raw);
        let what = question.what.clone();
        match question.kind {
            Kind::Def => graphcmd::def(ctx.graph()?, &[what], &mut out),
            Kind::Callers => graphcmd::callers(ctx.graph()?, &[what], &mut out),
            Kind::About => graphcmd::about(ctx.graph()?, &[what], &mut out),
            Kind::Affected => graphcmd::affected(ctx.graph()?, &what, DEFAULT_DEPTH, &mut out),
            Kind::Explain => graphcmd::explain(ctx.graph()?, &what, &mut out),
            Kind::Path => {
                let (from, to) = what.split_once('>').unwrap_or((&what, ""));
                graphcmd::path(ctx.graph()?, from.trim(), to.trim(), &mut out);
            }
            Kind::Find => findcmd::run(ctx, &FindArgs { query: what, k: DEFAULT_FIND_K, mode: Mode::Hybrid }, &mut out)?,
            Kind::Recall => memcmd::recall(ctx, &RecallArgs::new(what), &mut out)?,
        }
    }
    if ctx.engines.is_on(Engine::Memory) && !questions.iter().any(|q| q.kind == Kind::Recall) && ctx.bank_has_notes() {
        let text = questions.iter().map(Question::words).collect::<Vec<_>>().join(" ");
        let notes = memcmd::relevant_notes(ctx, &text, MEMORY_NOTES, MEMORY_TOKENS)?;
        if !notes.is_empty() {
            let now = ctx.now();
            ln!(out, "== memory");
            for note in &notes {
                ln!(out, "{}", memcmd::note_line(note, now));
            }
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn questions_split_at_the_first_colon() {
        let qs = parse(&args(&["def:a.py::foo", "find:where tokens expire", "path:A>B"])).unwrap();
        assert_eq!(qs[0].kind, Kind::Def);
        assert_eq!(qs[0].what, "a.py::foo");
        assert_eq!(qs[1].kind, Kind::Find);
        assert_eq!(qs[1].what, "where tokens expire");
        assert_eq!(qs[2].words(), "A B");
    }

    #[test]
    fn bad_questions_are_usage_errors() {
        for bad in [vec!["nokind"], vec!["what:x"], vec!["def:"], vec!["path:A"], vec!["path:>B"]] {
            let fail = parse(&args(&bad)).err().unwrap_or_else(|| panic!("{bad:?}"));
            assert_eq!(fail.code(), 2);
        }
    }
}
