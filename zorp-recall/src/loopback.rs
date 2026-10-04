//! The guard is `zorp-loopback`, the one copy `zorp-voice` uses too. What
//! lives here is how this crate's refusals read, because only this crate
//! knows the thing being refused is somebody's conversation text.
//!
//! The other two layers, `redirects(0)` and `try_proxy_from_env(false)`,
//! are on the HTTP agent in `embed.rs`, next to the resolver it is handed.

use zorp_loopback::{Phrases, Wording};

/// Conversation text, in the guard's sentences.
#[derive(Debug, Clone, Copy)]
pub struct RecallWording;

impl Wording for RecallWording {
    const PHRASES: &'static Phrases = &Phrases {
        endpoint: "endpoint",
        not_scheme_for: "an embedding endpoint",
        refusing_to: "embed conversations at",
        only_ever: "Conversation text is only ever sent to this machine",
        pinned_before: "this build only talks to ",
        pinned_after: ", because conversation text never leaves this machine",
    };
}

pub use zorp_loopback::LoopbackError;

/// An endpoint that passed the guard, worded for conversation text.
pub type LoopbackUrl = zorp_loopback::LoopbackUrl<RecallWording>;

/// The only resolver the embedder's HTTP client gets.
pub type LoopbackResolver = zorp_loopback::LoopbackResolver<RecallWording>;
