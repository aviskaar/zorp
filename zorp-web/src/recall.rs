//! Semantic search over the conversations already in the store.
//!
//! The whole implementation is `zorp_agent::recall` now. It moved because
//! the terminal could not search its own conversations at all, even though
//! the index is over a store both surfaces share, and copying it would have
//! given the workspace two chunkers and two fingerprints. The day those
//! disagreed the index would quietly hold two conventions.
//!
//! It is in `zorp-agent` rather than in `zorp-recall` because `zorp-recall`
//! deliberately depends on no other workspace member, the way `zorp-search`
//! and `zorp-skill` do, and the store the chunker reads is in `zorp-agent`.
//!
//! Re-exported here under the names this crate has always used, so nothing
//! that reads it had to move. The rule the whole thing rests on is
//! unchanged and is stated where it is enforced: conversation text goes to
//! a loopback address or it goes nowhere.

pub use zorp_agent::recall::*;
