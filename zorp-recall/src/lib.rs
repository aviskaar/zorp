//! Semantic search over stored conversations, on the machine they are
//! stored on.
//!
//! Three pieces, and the order they are in matters.
//!
//! - [`LoopbackUrl`] decides whether an endpoint is on this device, and
//!   [`LoopbackResolver`] is what keeps a connection there.
//! - [`OllamaEmbedder`] turns text into a vector by asking a local model.
//! - [`Index`] keeps the vectors in a SQLite file and answers queries.
//!
//! This crate depends on no other workspace member. It is handed the text
//! to index rather than reading a store, so it knows nothing about
//! sessions, agents, or the web server, and nothing about it has to change
//! when they do. `zorp-web` is the caller that knows a conversation is a
//! row in `zorp-agent`'s store.
//!
//! # The one rule
//!
//! Conversation text goes to a loopback address or it goes nowhere. There
//! is no remote provider here, no flag that adds one, and no fallback when
//! the local model is missing: a missing embedder is an error that says so.
//! The corpus is a person's whole history with an agent that reads their
//! files, and a capability that quietly ships that to an API in order to
//! keep working is not a degraded version of this feature, it is the worst
//! thing this code could do.
//!
//! `tests/no_remote.rs` and `tests/no_proxy.rs` are where that is pinned.
//! Each one points a would-be escape at a loopback socket that counts
//! connections and passes only when the count is zero, because an error and
//! a request that was never made look the same from the caller's side.

mod embed;
mod index;
mod loopback;

pub use embed::{
    EmbedError, Embedder, OllamaEmbedder, DEFAULT_EMBED_MODEL, DEFAULT_EMBED_URL, EMBED_MODEL_VAR,
    EMBED_URL_VAR,
};
pub use index::{Chunk, Hit, Index, IndexError, Stats};
pub use loopback::{LoopbackError, LoopbackResolver, LoopbackUrl};
