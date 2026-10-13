//! Shared types and storage helpers for the lxr engines.
//!
//! `model` is the contract between the crates: `lxr-extract` produces [`FileFacts`],
//! and the graph, search and memory crates consume them. Nothing here parses code.

pub mod model;
pub mod store;

pub use model::{Call, FileFacts, Import, Lang, ParseGrade, SymKind, Symbol};
pub use store::{est_tokens, find_root, hash64, index_dir, index_home, load, repo_key, save};
