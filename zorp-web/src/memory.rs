//! Putting an older conversation in front of the model working on this one.
//!
//! The whole implementation is `zorp_agent::memory` now. It moved with
//! `recall`, because both surfaces read the same index the same way and a
//! terminal turn could not recall anything at all. Copying it would have
//! given the workspace two fences, two nonces and two boundary sentences
//! over the same untrusted text.
//!
//! Re-exported here under the names this crate has always used. The four
//! rules are unchanged and are stated where they are enforced: the unit is
//! a verbatim message, an assistant line is labelled as model output,
//! recalled text is fenced data in a `user` message, and the block is
//! appended to the seed so it reaches the model and never the store.

pub use zorp_agent::memory::*;
