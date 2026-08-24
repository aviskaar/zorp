//! Loopback-only voice transcription for zorp.

mod client;
mod loopback;
mod setup;

pub use client::{
    QwenAsr, Transcription, VoiceError, VoiceStatus, DEFAULT_VOICE_MODEL, DEFAULT_VOICE_URL,
    VOICE_MODEL_VAR, VOICE_URL_VAR,
};
pub use loopback::{LoopbackError, LoopbackResolver, LoopbackUrl};
pub use setup::{
    BootstrapOutcome, BootstrapProgress, BootstrapStage, SetupBackend, SetupError, SetupProgress,
    SetupStage, VoiceSetup, QWEN_ASR_PACKAGE, QWEN_ASR_VLLM_PACKAGE, VOICE_AUTOSTART_VAR,
    VOICE_PYTHON_VAR, VOICE_SETUP_DIR_VAR,
};
