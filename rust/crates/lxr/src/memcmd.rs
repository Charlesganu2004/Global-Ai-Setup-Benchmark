//! The memory commands: retain, recall, reflect and `memory list|forget|stats|path`.

use crate::cli::{MemoryCmd, RecallArgs, RetainArgs};
use crate::ctx::{Ctx, Engine};
use crate::dates::age_word;
use crate::fail::{Fail, Res};
use crate::localtime;
use lxr_memory::{FlushError, Note, RecallOpts, Recalled, RetainOpts, RetainOutcome};
use std::collections::HashSet;

/// A note's text on one line.
fn flat(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// When a note was last written, in Unix seconds.
fn written(note: &Note) -> i64 {
    [note.updated_at, note.created_at, note.mentioned_at].into_iter().find(|t| *t > 0).unwrap_or(0)
}

/// `- TEXT  (AGE ago, seen N times) #ID`, with `, stale: code changed` or
/// `, gone: code removed` inside the brackets when the code the note names has moved.
pub fn note_line(recalled: &Recalled, now: i64) -> String {
    let note = &recalled.note;
    let mut inside = format!("{} ago", age_word(now - written(note)));
    if note.proof_count > 1 {
        inside.push_str(&format!(", seen {} times", note.proof_count));
    }
    if recalled.gone {
        inside.push_str(", gone: code removed");
    } else if recalled.stale {
        inside.push_str(", stale: code changed");
    }
    format!("- {}  ({inside}) #{}", flat(&note.text), note.id)
}

/// The recall options for a set of flags. Words such as "today" in the question mean the days of
/// the machine's time zone.
pub fn recall_opts(args: &RecallArgs) -> RecallOpts {
    RecallOpts {
        limit: args.k,
        budget: args.budget,
        max_tokens: args.max_tokens,
        tags: args.tags.clone(),
        since: args.since,
        until: args.until,
        utc_offset: localtime::utc_offset(),
        ..RecallOpts::default()
    }
}

/// The retain options for a set of flags. Words such as "today" in the text mean the days of the
/// machine's time zone.
pub fn retain_opts(args: &RetainArgs) -> RetainOpts {
    RetainOpts {
        kind: args.kind,
        context: args.context.clone(),
        tags: args.tags.clone(),
        when: args.when,
        split: !args.whole,
        utc_offset: localtime::utc_offset(),
        ..RetainOpts::default()
    }
}

/// What to do with the result of making a change to the bank durable. A log that could not be
/// made durable is an error: the change may be lost, and it must not be reported as made. A cache
/// that could not be written is a warning on standard error: the change is safe in the log, and
/// the next run builds the cache again.
fn settle(result: Result<(), FlushError>) -> Res<()> {
    match result {
        Ok(()) => Ok(()),
        Err(FlushError::Cache(error)) => {
            crate::say(&format!(
                "pgr: warning: the cache files of the memory bank could not be written ({error}); the change is safe in the log and the cache is built again on the next run"
            ));
            Ok(())
        }
        Err(FlushError::Log(error)) => Err(Fail::index(format!("cannot write the memory bank: {error}"))),
    }
}

/// `pgr retain TEXT...`.
pub fn retain(ctx: &mut Ctx, args: &RetainArgs, out: &mut String) -> Res<()> {
    ctx.need_memory()?;
    let opts = retain_opts(args);
    // The bank is brought up to date inside the lock, so it holds every note written until now.
    let _lock = ctx.lock_bank()?;
    let kept = ctx.bank_session(|bank, code, embedder| {
        let done = bank
            .retain(&args.text, &opts, code, embedder)
            .map_err(|e| Fail::index(format!("cannot write the memory bank: {e}")))?;
        // The flush makes the log durable: a note is not reported as kept before that.
        settle(bank.flush())?;
        Ok::<_, Fail>(
            done.into_iter()
                .map(|r| {
                    let seen = bank.get(&r.id).map(|n| n.proof_count).unwrap_or(1);
                    (r.id, r.outcome, seen)
                })
                .collect::<Vec<_>>(),
        )
    })??;
    if kept.is_empty() {
        ln!(out, "nothing kept");
    }
    for (id, outcome, seen) in kept {
        match outcome {
            RetainOutcome::Added => ln!(out, "kept {id}"),
            RetainOutcome::Reinforced => ln!(out, "reinforced {id} (seen {seen} times)"),
        }
    }
    Ok(())
}

/// The notes a recall gives for some flags.
pub fn recall_notes(ctx: &mut Ctx, args: &RecallArgs) -> Res<Vec<Recalled>> {
    ctx.need_memory()?;
    if !ctx.bank_dir.is_dir() {
        return Ok(Vec::new());
    }
    let opts = recall_opts(args);
    ctx.bank_session(|bank, code, embedder| bank.recall(&args.query, &opts, code, embedder))
}

/// `pgr recall WORDS...`.
pub fn recall(ctx: &mut Ctx, args: &RecallArgs, out: &mut String) -> Res<()> {
    let notes = recall_notes(ctx, args)?;
    if notes.is_empty() {
        ln!(out, "no notes");
    }
    let now = ctx.now();
    for note in &notes {
        ln!(out, "{}", note_line(note, now));
    }
    Ok(())
}

/// `pgr reflect WORDS...`: the notes sorted into the parts an agent reads in order.
pub fn reflect(ctx: &mut Ctx, query: &str, out: &mut String) -> Res<()> {
    ctx.need_memory()?;
    let now = ctx.now();
    let opts = recall_opts(&RecallArgs { query: query.to_string(), ..RecallArgs::new(String::new()) });
    let reflection = if ctx.bank_dir.is_dir() {
        ctx.bank_session(|bank, code, embedder| bank.reflect(query, &opts, code, embedder))?
    } else {
        Default::default()
    };
    let sections: [(&str, &Vec<Recalled>); 3] =
        [("directives:", &reflection.directives), ("observations:", &reflection.observations), ("notes:", &reflection.facts)];
    let mut printed = false;
    for (title, notes) in sections {
        if notes.is_empty() {
            continue;
        }
        printed = true;
        ln!(out, "{title}");
        for note in notes {
            ln!(out, "{}", note_line(note, now));
        }
    }
    if !reflection.by_entity.is_empty() {
        printed = true;
        ln!(out, "by subject:");
        for (entity, ids) in &reflection.by_entity {
            let tags: Vec<String> = ids.iter().map(|id| format!("#{id}")).collect();
            ln!(out, "  {entity}: {}", tags.join(" "));
        }
    }
    if !printed {
        ln!(out, "no notes");
    }
    Ok(())
}

/// `pgr memory list|forget|stats|path`. None of these reads the code index.
pub fn memory(ctx: &mut Ctx, command: &MemoryCmd, out: &mut String) -> Res<()> {
    ctx.engines.require(Engine::Memory)?;
    let now = ctx.now();
    match command {
        MemoryCmd::Path => ln!(out, "{}", ctx.bank_dir.display()),
        MemoryCmd::List { k } => {
            if !ctx.bank_dir.is_dir() {
                ln!(out, "no notes");
                return Ok(());
            }
            let bank = ctx.bank()?;
            let notes = bank.notes();
            if notes.is_empty() {
                ln!(out, "no notes");
            }
            for note in notes.into_iter().take(*k) {
                let mut inside = format!("{} ago, {}", age_word(now - written(note)), note.kind.name());
                if note.proof_count > 1 {
                    inside.push_str(&format!(", seen {} times", note.proof_count));
                }
                ln!(out, "- {}  ({inside}) #{}", flat(&note.text), note.id);
            }
        }
        MemoryCmd::Forget { id } => {
            let id = id.trim_start_matches('#');
            let forgotten = if ctx.bank_dir.is_dir() {
                let _lock = ctx.lock_bank()?;
                let bank = ctx.bank()?;
                let done = bank.forget(id).map_err(|e| Fail::index(format!("cannot write the memory bank: {e}")))?;
                settle(bank.flush())?;
                done
            } else {
                false
            };
            if forgotten {
                ln!(out, "forgot {id}");
            } else {
                ln!(out, "no note {id}");
            }
        }
        MemoryCmd::Stats => {
            let stats = if ctx.bank_dir.is_dir() { Some(ctx.bank()?.stats()) } else { None };
            let stats = stats.unwrap_or_default();
            ln!(
                out,
                "{} notes (world {}, experience {}, observation {}, directive {}), {} entities, {} links, {} bytes",
                stats.notes,
                stats.by_kind[0],
                stats.by_kind[1],
                stats.by_kind[2],
                stats.by_kind[3],
                stats.entities,
                stats.links,
                stats.bytes
            );
        }
    }
    Ok(())
}

/// The recall options `ask` and `pack` use for the notes that go with an answer.
pub fn companion_opts(limit: usize, max_tokens: usize, query: &str) -> RecallArgs {
    RecallArgs { k: limit, max_tokens, ..RecallArgs::new(query.to_string()) }
}

/// Words too common to show that a note is about a question.
const COMMON_WORDS: &[&str] = &[
    "the", "and", "for", "with", "that", "this", "are", "was", "were", "how", "what", "where", "when", "which", "who", "why",
    "from", "into", "not", "you", "your", "can", "does", "did", "should", "would", "could", "all", "any", "its", "has", "have",
    "had", "but", "use", "used", "using", "about", "there", "their", "then", "than", "also", "been", "will", "may", "must",
];

/// The lower-case words of a text that can show a note is about it: at least three
/// letters or digits, not a common word. `load_config` gives `load` and `config`.
fn telling_words(text: &str) -> HashSet<String> {
    text.split(|c: char| !c.is_alphanumeric())
        .map(|w| w.to_lowercase())
        .filter(|w| w.chars().count() >= 3 && !COMMON_WORDS.contains(&w.as_str()))
        .collect()
}

/// True when the note and the question share a telling word, in the note's text, its
/// entities or the names of the definitions it is anchored to.
fn shares_a_word(question: &HashSet<String>, note: &Note) -> bool {
    let mut own = telling_words(&note.text);
    for entity in &note.entities {
        own.extend(telling_words(entity));
    }
    for anchor in &note.anchors {
        own.extend(telling_words(&anchor.qual));
    }
    own.iter().any(|w| question.contains(w))
}

/// The notes that go with an answer: the bank's recall for the text, without those that
/// share no telling word with it (the bank also offers notes that are only recent or
/// linked to others). Empty when no note qualifies.
pub fn relevant_notes(ctx: &mut Ctx, query: &str, limit: usize, max_tokens: usize) -> Res<Vec<Recalled>> {
    let args = companion_opts(limit, max_tokens, query);
    let notes = recall_notes(ctx, &args)?;
    let question = telling_words(query);
    Ok(notes.into_iter().filter(|r| shares_a_word(&question, &r.note)).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use lxr_memory::Kind;

    fn failure() -> std::io::Error {
        std::io::Error::new(std::io::ErrorKind::PermissionDenied, "access is denied")
    }

    #[test]
    fn a_log_that_cannot_be_made_durable_is_an_error_and_a_cache_that_cannot_be_written_is_not() {
        assert!(settle(Ok(())).is_ok());
        let error = settle(Err(FlushError::Log(failure()))).unwrap_err();
        assert_eq!(error.code(), 1);
        assert!(error.text().starts_with("pgr: cannot write the memory bank: "), "{}", error.text());
        assert!(error.text().contains("access is denied"));
        // A warning goes to standard error and the command goes on: the note is safe in the log.
        assert!(settle(Err(FlushError::Cache(failure()))).is_ok());
    }

    #[test]
    fn the_options_carry_the_offset_of_the_machine() {
        let offset = localtime::utc_offset();
        let retain = RetainArgs { text: "x".into(), kind: Kind::World, tags: Vec::new(), context: String::new(), when: None, whole: false };
        assert_eq!(retain_opts(&retain).utc_offset, offset);
        assert_eq!(recall_opts(&RecallArgs::new("x".to_string())).utc_offset, offset);
        // The companion recalls of ask and pack go through the same options.
        assert_eq!(recall_opts(&companion_opts(4, 200, "what is today")).utc_offset, offset);
        // The other fields of the flags are still carried over.
        let flagged = RetainArgs { kind: Kind::Directive, tags: vec!["t".into()], whole: true, when: Some(5), ..retain };
        let opts = retain_opts(&flagged);
        assert_eq!((opts.kind, opts.when, opts.split), (Kind::Directive, Some(5), false));
    }

    #[test]
    fn telling_words_split_identifiers_and_drop_common_words() {
        let words = telling_words("How does load_config read the file?");
        assert!(words.contains("load") && words.contains("config") && words.contains("read") && words.contains("file"));
        assert!(!words.contains("how") && !words.contains("the") && !words.contains("does"));
    }

    #[test]
    fn a_note_qualifies_through_text_entities_or_anchors() {
        let question = telling_words("what calls load_config");
        let mut note = Note { text: "Always run the formatter.".to_string(), ..Note::default() };
        assert!(!shares_a_word(&question, &note));
        note.text = "The config file is read once.".to_string();
        assert!(shares_a_word(&question, &note));
        note.text = "unrelated".to_string();
        note.entities = vec!["Load".to_string()];
        assert!(shares_a_word(&question, &note));
    }
}
