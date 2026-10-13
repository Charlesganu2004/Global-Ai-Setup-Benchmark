//! Notes that outlive a session, recalled by meaning, words, links and time.
//!
//! A [`Bank`] is a folder of notes for one repository. It is a local, model-free
//! reimplementation of the ideas of the open-source memory system Hindsight: facts are
//! cut out of text, their entities are resolved, they are linked by time, meaning and
//! cause, and a recall runs four strategies (meaning, words, links, time) whose ranked
//! lists are fused. Nothing here calls a language model or the network.
//!
//! One addition is ours: a note knows which definitions in the code it talks about, as
//! [`Anchor`]s with the hash of the source at the time. A recall asks a [`CodeView`] for the
//! hash now and marks the note `stale` when the code changed or `gone` when the definition
//! disappeared, and ranks it lower, so that a note written before the code changed is not
//! trusted as if it were current.
//!
//! The folder holds three files. `notes.jsonl` is the source of truth: one JSON object per
//! line, a change appends the note's new full state, a forget appends a tombstone, and the
//! last line for an id wins. `vectors.bin` and `index.bin` are caches that are rebuilt from
//! it when they are missing, damaged or older than it.

mod bank;
mod dates;
mod entities;
mod graph;
mod log;
mod recall;
mod split;
mod store;
#[cfg(test)]
mod testutil;
#[cfg(test)]
mod tests;

pub use bank::{Bank, FlushError};
pub use recall::Strategies;

use serde::{Deserialize, Serialize};

/// Cosine at or above which a new fact counts as a repeat of an older note of the same kind
/// whose entities overlap. Measured on 200 docstring notes with the 256-dimension static code
/// model (see the `calibrate` example): lightly edited copies of a note (an article dropped, a
/// word added or swapped, a clause appended) score 0.97 to 1.00, copies with the last two
/// words cut score 0.95 or more for nine in ten, and the most similar pair of different notes
/// among all 19,900 scores 0.88 (a "remove" and a "write" docstring of two sibling functions).
///
/// The three thresholds were measured once, on a `DOCSTRINGS.json` that `pick_docstrings.py`
/// wrote. Neither file is in this repository, so the figures here cannot be regenerated or
/// checked from it: treat the values as defaults. `calibrate` and `recallbench` describe the
/// format of the input they read.
pub const NEAR_DUPLICATE: f32 = 0.95;
/// Cosine at or above which two notes get a semantic link. Measured with `calibrate` on the
/// same docstring set (see [`NEAR_DUPLICATE`]): random pairs of docstring notes reach 0.57 at
/// the 99th percentile and 0.72 at the 99.9th. At 0.60 a note has 1.3 links among 200 notes,
/// and 65% of the linked pairs are from one file (4% of all pairs are), 25% share three
/// content words (0.6% of all pairs do).
pub const LINK_THRESHOLD: f32 = 0.60;
/// Cosine below which the meaning strategy drops a note. Measured with `calibrate` on 60
/// queries of half the words of a docstring sentence, from the same set (see
/// [`NEAR_DUPLICATE`]): the right note scores at least 0.21 and 98% of them 0.30 or more,
/// while a wrong note passes 0.30 in 3% of the cases.
pub const MEANING_FLOOR: f32 = 0.30;

/// What a note is.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
pub enum Kind {
    /// A fact about the world, the code or the project.
    #[default]
    World,
    /// Something that happened to the agent or was done by it.
    Experience,
    /// A summary the bank built itself from three or more facts about one entity. Entities
    /// that are about exactly the same facts (a file and the function in it, say) share one
    /// observation, whose `entities` lists all of them.
    Observation,
    /// A standing rule. Recall returns the directives that pass the filters first, though never
    /// more than half of its limit and half of its token allowance while notes that answer the
    /// query are waiting; what those notes leave unused goes to the remaining directives.
    Directive,
}

impl Kind {
    /// Position of the kind in [`BankStats::by_kind`].
    pub fn index(self) -> usize {
        match self {
            Kind::World => 0,
            Kind::Experience => 1,
            Kind::Observation => 2,
            Kind::Directive => 3,
        }
    }

    /// The lowercase word for the kind: `world`, `experience`, `observation`, `directive`.
    pub fn name(self) -> &'static str {
        match self {
            Kind::World => "world",
            Kind::Experience => "experience",
            Kind::Observation => "observation",
            Kind::Directive => "directive",
        }
    }

    /// The kind a word names, in any case.
    pub fn from_name(word: &str) -> Option<Kind> {
        match word.trim().to_ascii_lowercase().as_str() {
            "world" => Some(Kind::World),
            "experience" => Some(Kind::Experience),
            "observation" => Some(Kind::Observation),
            "directive" => Some(Kind::Directive),
            _ => None,
        }
    }
}

/// A definition a note talks about, with the hash of its source at the time of writing.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Hash, Debug, Default)]
pub struct Anchor {
    /// Path of the file from the repository root, with forward slashes.
    pub path: String,
    /// The definition's qualified name inside the file, such as `Config.parse`.
    pub qual: String,
    /// Hash of the definition's source lines when the note was written.
    pub hash: u64,
}

/// What the bank asks about the code a note mentions.
pub trait CodeView {
    /// The definition a word names, when exactly one does.
    fn anchor(&self, word: &str) -> Option<Anchor>;
    /// That definition's hash now, or `None` when it is gone.
    fn current_hash(&self, anchor: &Anchor) -> Option<u64>;
    /// False for a view that knows no code at all. Recall then does not judge anchors,
    /// because "I know nothing" is not the same as "the definition is gone".
    fn knows_code(&self) -> bool {
        true
    }
}

/// A [`CodeView`] that knows no code: no word names a definition and no note is judged.
#[derive(Clone, Copy, Debug, Default)]
pub struct NoCode;

impl CodeView for NoCode {
    fn anchor(&self, _word: &str) -> Option<Anchor> {
        None
    }

    fn current_hash(&self, _anchor: &Anchor) -> Option<u64> {
        None
    }

    fn knows_code(&self) -> bool {
        false
    }
}

/// One remembered thing. Times are Unix seconds, UTC.
#[derive(Serialize, Deserialize, Clone, PartialEq, Debug, Default)]
pub struct Note {
    /// Stable id, such as `n000042`. Never reused, even after a forget.
    pub id: String,
    pub kind: Kind,
    /// The fact itself.
    #[serde(default)]
    pub text: String,
    /// Where or why it was said, as the caller gave it.
    #[serde(default)]
    pub context: String,
    /// Labels used to filter a recall.
    #[serde(default)]
    pub tags: Vec<String>,
    /// Names the note is about, after spelling variants were merged.
    #[serde(default)]
    pub entities: Vec<String>,
    /// Definitions the note talks about, with their hash when it was written.
    #[serde(default)]
    pub anchors: Vec<Anchor>,
    /// Start of the time the fact is about.
    #[serde(default)]
    pub occurred_start: Option<i64>,
    /// End of the time the fact is about.
    #[serde(default)]
    pub occurred_end: Option<i64>,
    /// When the note was retained.
    #[serde(default)]
    pub mentioned_at: i64,
    #[serde(default)]
    pub created_at: i64,
    #[serde(default)]
    pub updated_at: i64,
    /// How many times the fact was seen: 1 when new, plus one per repeat. For an
    /// observation, the number of notes it summarises.
    #[serde(default = "one")]
    pub proof_count: u32,
    /// For an observation, the ids of the notes it summarises.
    #[serde(default)]
    pub sources: Vec<String>,
    /// One line per change: repeats and refreshes, oldest first.
    #[serde(default)]
    pub history: Vec<String>,
    /// Ids of the notes that, by a cue phrase, explain this one or that it explains.
    #[serde(default)]
    pub caused_by: Vec<String>,
}

fn one() -> u32 {
    1
}

/// How to retain a text.
#[derive(Clone, Debug)]
pub struct RetainOpts {
    pub kind: Kind,
    pub context: String,
    pub tags: Vec<String>,
    /// Entities to add to those found in the text.
    pub entities: Vec<String>,
    /// When the fact happened, Unix seconds. Overrides a date found in the text.
    pub when: Option<i64>,
    /// The clock, Unix seconds. The system clock when `None`; tests set it.
    pub now: Option<i64>,
    /// Cut the text into facts. When false the whole text is one note.
    pub split: bool,
    /// The time zone of the caller, in seconds east of UTC (negative to the west). Words such
    /// as `today`, `yesterday` and `last week` in a text mean the days and weeks of that zone.
    /// 0, the default, reads them in UTC. A value beyond a day either way is cut to a day.
    pub utc_offset: i64,
}

impl Default for RetainOpts {
    fn default() -> RetainOpts {
        RetainOpts {
            kind: Kind::World,
            context: String::new(),
            tags: Vec::new(),
            entities: Vec::new(),
            when: None,
            now: None,
            split: true,
            utc_offset: 0,
        }
    }
}

/// What happened to one fact that was retained.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RetainOutcome {
    /// A new note was written.
    Added,
    /// The bank already held the fact: its proof count went up.
    Reinforced,
}

/// The result for one fact.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Retained {
    /// The id of the new note, or of the note that was reinforced.
    pub id: String,
    pub outcome: RetainOutcome,
}

/// How many candidates each strategy of a recall may return.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Budget {
    /// 100 candidates per strategy.
    Low,
    /// 300 candidates per strategy.
    Mid,
    /// 1000 candidates per strategy.
    High,
}

impl Budget {
    /// Candidates per strategy.
    pub fn count(self) -> usize {
        match self {
            Budget::Low => 100,
            Budget::Mid => 300,
            Budget::High => 1000,
        }
    }

    /// The next larger budget; the largest stays.
    pub fn up(self) -> Budget {
        match self {
            Budget::Low => Budget::Mid,
            Budget::Mid | Budget::High => Budget::High,
        }
    }
}

/// How to recall.
#[derive(Clone, Debug)]
pub struct RecallOpts {
    /// Most notes returned.
    pub limit: usize,
    pub budget: Budget,
    /// Estimated tokens the returned texts may add up to.
    pub max_tokens: usize,
    /// Only these kinds; every kind when empty.
    pub kinds: Vec<Kind>,
    /// Only notes with these tags; no filter when empty.
    pub tags: Vec<String>,
    /// With `tags`: a note must carry all of them instead of any one.
    pub tags_all: bool,
    /// Only notes whose time reaches this far (Unix seconds).
    pub since: Option<i64>,
    /// Only notes whose time starts no later than this (Unix seconds).
    pub until: Option<i64>,
    /// The clock, Unix seconds. The system clock when `None`.
    pub now: Option<i64>,
    /// The time zone of the caller, in seconds east of UTC (negative to the west). Words such
    /// as `today`, `yesterday` and `last week` in the query mean the days and weeks of that
    /// zone. 0, the default, reads them in UTC. A value beyond a day either way is cut to a day.
    /// `since` and `until` are instants and are not changed by it.
    pub utc_offset: i64,
}

impl Default for RecallOpts {
    fn default() -> RecallOpts {
        RecallOpts {
            limit: 8,
            budget: Budget::Mid,
            max_tokens: 800,
            kinds: Vec::new(),
            tags: Vec::new(),
            tags_all: false,
            since: None,
            until: None,
            now: None,
            utc_offset: 0,
        }
    }
}

/// One note returned by a recall.
#[derive(Clone, PartialEq, Debug)]
pub struct Recalled {
    pub note: Note,
    /// Rank score after the recency, time and proof boosts and the stale and gone demotion.
    pub score: f32,
    /// An anchor's definition has changed since the note was written.
    pub stale: bool,
    /// An anchor's definition no longer exists.
    pub gone: bool,
    /// The strategies that found it: `meaning`, `words`, `links`, `time`.
    pub via: Vec<&'static str>,
}

/// A recall sorted into the parts an agent reads in order.
#[derive(Clone, PartialEq, Debug, Default)]
pub struct Reflection {
    pub directives: Vec<Recalled>,
    pub observations: Vec<Recalled>,
    /// World and experience notes.
    pub facts: Vec<Recalled>,
    /// Entity name and the ids of the returned notes that mention it, most mentioned first.
    pub by_entity: Vec<(String, Vec<String>)>,
}

/// What a bank holds.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct BankStats {
    /// Notes that were not forgotten.
    pub notes: usize,
    /// Notes per kind, in the order of [`Kind::index`].
    pub by_kind: [usize; 4],
    /// Distinct entities that at least one note mentions.
    pub entities: usize,
    /// Stored links: temporal, semantic and causal.
    pub links: usize,
    /// Size of the bank's files on disk.
    pub bytes: u64,
}

/// Asks the code view about each definition a note names: `(stale, gone)`. `stale` when one has
/// changed since the note was written, `gone` when one no longer exists. A note that names no
/// code, and a view that knows no code, give `(false, false)`. This is the judgement a recall
/// passes on every note it returns, for a caller that lists notes without recalling them.
pub fn judge_note(note: &Note, code: &dyn CodeView) -> (bool, bool) {
    recall::judge(note, code)
}

/// The entities the rules find in a text, without the ones a caller passes with it. For
/// tools that want to see what [`Bank::retain`] would extract.
pub fn find_entities(text: &str) -> Vec<String> {
    entities::extract(text)
}

/// The text a note is embedded as: `TEXT (happened DATE) [entity, entity]`. For tools that
/// measure the embedding of notes.
pub fn embedding_text(note: &Note) -> String {
    bank::embed_text(note)
}
