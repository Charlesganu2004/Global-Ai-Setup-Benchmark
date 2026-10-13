//! The code graph: definitions, calls, imports, and the questions asked of them.
//!
//! [`Graph::build`] takes the facts of every file of a repository (see
//! `lxr_core::FileFacts`) and ties them together:
//!
//! - one node per file and one per definition;
//! - `Contains` and `Method` edges from the structure of each file;
//! - `Imports` edges between files, found by resolving each import to a file;
//! - `Inherits` edges from each class to its bases;
//! - `Calls` edges from each caller to each definition a call can be tied to, with a
//!   confidence, and a list of calls that no rule could tie to anything.
//!
//! The questions ([`Graph::find`], [`Graph::callers`], [`Graph::affected`],
//! [`Graph::shortest_path`], [`Graph::hubs`], [`Graph::communities`], [`Graph::query`])
//! all run on the built graph, with no model and no network.

mod build;
mod community;
mod lookup;
mod modules;
mod query;
mod resolve;
mod types;
mod walk;

#[cfg(test)]
mod testkit;

pub use types::{
    Affected, Confidence, Edge, Graph, Hop, Link, LooseCall, Node, NodeKind, QueryAnswer, Relation, Stats,
};
