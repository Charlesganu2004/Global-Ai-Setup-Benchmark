//! The switches of the search that the environment turns on. All of them are off unless set, and
//! with none set every command prints what pgr 0.2 printed.
//!
//! - `PGR_SEARCH_WEIGHTS="names=1,words=1,meaning=1,k=60,..."`: the numbers of the fusion
//!   (see `lxr_rag::WEIGHT_KEYS`).
//! - `PGR_SEARCH_REFINE=N`: a second pass over the best N hits, which adds their rarest words to
//!   the query at a lower weight (0 is off).
//! - `PGR_SEARCH_VECTORS=model|repo|both`: where the vectors of the meaning channel come from. The
//!   model is the default; `repo` makes them from the repository's own word co-occurrence, with no
//!   model; `both` fuses the two. For `repo` and `both`, an index built with the switch on keeps
//!   the vectors in a file beside it, and one built without makes them again for every query.
//! - `PGR_SEARCH_GRAPH=1`: a hit that is a test, or a thin wrapper, is followed by the definitions
//!   it calls (see `findcmd`). `0` or empty is off; any other value is a usage error.

use crate::fail::{Fail, Res};
use lxr_rag::{Meaning, Tuning};

/// The value of an environment variable, if it is set.
fn var(name: &str) -> Option<String> {
    std::env::var_os(name).map(|v| v.to_string_lossy().into_owned())
}

/// The tuning the environment asks for. A value that cannot be understood is a usage error:
/// a misspelt switch must not quietly give the answers of the default search.
pub fn tuning() -> Res<Tuning> {
    Tuning::parse(var("PGR_SEARCH_WEIGHTS").as_deref(), var("PGR_SEARCH_REFINE").as_deref(), var("PGR_SEARCH_VECTORS").as_deref())
        .map_err(Fail::usage)
}

/// True when `PGR_SEARCH_GRAPH` is `1`; nothing, empty and `0` are off. Any other value is a usage
/// error, like a value of the other switches that cannot be understood.
pub fn graph_on() -> Res<bool> {
    match var("PGR_SEARCH_GRAPH").as_deref().map(str::trim) {
        None | Some("") | Some("0") => Ok(false),
        Some("1") => Ok(true),
        Some(other) => Err(Fail::usage(format!("PGR_SEARCH_GRAPH: '{other}' is not 0 or 1"))),
    }
}

/// True when the search needs vectors made from the repository, so that an index being built
/// should keep them. A value that cannot be understood counts as no.
pub fn wants_repo_vectors() -> bool {
    tuning().is_ok_and(|t| t.uses_repo_vectors())
}

/// True when the search leaves the embedding model alone, so that it need not be opened.
pub fn leaves_model_alone() -> bool {
    tuning().is_ok_and(|t| t.meaning == Meaning::Repo)
}
