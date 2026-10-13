//! Code-aware tokens, BM25, static embeddings and rank fusion.
//!
//! This is the text engine of the code index. It does in Rust what semble does with
//! model2vec, numpy and vicinity: [`code_tokens`] splits source text into identifier
//! tokens, [`Bm25`] ranks documents by them, [`Embedder`] turns text into a unit vector
//! with a static WordPiece model, [`Vectors`] searches a matrix of such vectors, and
//! [`rrf`] merges ranked lists. [`trigram_similarity`] compares names.

mod bm25;
mod embed;
mod fusion;
mod tokenizer;
mod tokens;
mod unicode_data;
mod vectors;

pub use bm25::Bm25;
pub use embed::Embedder;
pub use fusion::{rrf, trigram_similarity};
pub use tokens::code_tokens;
pub use vectors::Vectors;
