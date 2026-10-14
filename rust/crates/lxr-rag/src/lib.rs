//! Hybrid code search over definition-sized chunks.
//!
//! [`chunk_file`] cuts a file into chunks that follow its definitions. [`SearchIndex`]
//! indexes chunks twice, by code-aware words (BM25) and by static embeddings, and
//! [`SearchIndex::search`] fuses the two rankings, applies semble's boosts and path
//! penalties, and reports each hit as the definition it lies in. This is the Rust
//! replacement for semble 0.6.1.

mod chunk;
mod index;
mod rank;
mod repo;
mod tuning;

pub use chunk::{chunk_file, Chunk, ChunkKind};
pub use index::{Hit, Mode, SearchIndex, SearchOpts};
pub use rank::is_test_path;
pub use tuning::{Meaning, Tuning, Weights, WEIGHT_KEYS};
