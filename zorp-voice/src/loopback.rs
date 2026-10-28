//! The guard is `zorp-loopback`, the one copy `zorp-recall` uses too. What
//! lives here is how this crate's refusals read, because only this crate
//! knows the thing being refused is a recording of somebody's voice, and
//! the one question about an endpoint that is voice's alone: whether
//! `qwen-asr-serve` can bind it directly.
//!
//! The other two layers, `redirects(0)` and `try_proxy_from_env(false)`,
//! are on the HTTP agent in `client.rs`, next to the resolver it is handed.

use zorp_loopback::{Phrases, Wording};

/// Recorded audio, in the guard's sentences.
#[derive(Debug, Clone, Copy)]
pub struct VoiceWording;

impl Wording for VoiceWording {
    const PHRASES: &'static Phrases = &Phrases {
        endpoint: "voice endpoint",
        not_scheme_for: "a voice endpoint",
        refusing_to: "send recorded audio to",
        only_ever: "Voice is only ever sent to this machine",
        pinned_before: "voice is pinned to ",
        pinned_after: "",
    };
}

/// Text to be spoken, in the guard's sentences. A spoken answer can quote a
/// person's files, so the text going to a TTS runtime gets the same boundary
/// as recorded audio, worded for what it is carrying.
#[derive(Debug, Clone, Copy)]
pub struct SpeechWording;

impl Wording for SpeechWording {
    const PHRASES: &'static Phrases = &Phrases {
        endpoint: "speech endpoint",
        not_scheme_for: "a speech endpoint",
        refusing_to: "send text to be spoken to",
        only_ever: "Text to speak is only ever sent to this machine",
        pinned_before: "speech is pinned to ",
        pinned_after: "",
    };
}

pub use zorp_loopback::LoopbackError;

/// An endpoint that passed the guard, worded for recorded audio.
pub type LoopbackUrl = zorp_loopback::LoopbackUrl<VoiceWording>;

/// The only resolver the voice client's HTTP client gets.
pub type LoopbackResolver = zorp_loopback::LoopbackResolver<VoiceWording>;

/// An endpoint that passed the guard, worded for text to be spoken.
pub type SpeechUrl = zorp_loopback::LoopbackUrl<SpeechWording>;

/// The only resolver the speech client's HTTP client gets.
pub type SpeechResolver = zorp_loopback::LoopbackResolver<SpeechWording>;

/// What voice needs to know about a checked endpoint that the guard does
/// not: whether `qwen-asr-serve` can bind it without an operator proxy.
pub trait DirectRuntime {
    /// Plain `http` with no path. HTTPS and a path prefix both need an
    /// operator-managed loopback proxy in front of the runtime.
    fn supports_direct_runtime(&self) -> bool;
}

impl DirectRuntime for LoopbackUrl {
    fn supports_direct_runtime(&self) -> bool {
        self.scheme() == "http" && self.path().is_empty()
    }
}
