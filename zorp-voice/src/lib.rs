//! Loopback-only voice transcription for zorp.

mod client;
mod loopback;

pub use client::{
    QwenAsr, Transcription, VoiceError, VoiceStatus, DEFAULT_VOICE_MODEL, DEFAULT_VOICE_URL,
    VOICE_MODEL_VAR, VOICE_URL_VAR,
};
pub use loopback::{LoopbackError, LoopbackResolver, LoopbackUrl};
