//! Loopback-only voice transcription for zorp.

mod client;
mod loopback;
mod setup;
mod speech;

pub use client::{
    QwenAsr, Transcription, VoiceError, VoiceStatus, DEFAULT_VOICE_MODEL, DEFAULT_VOICE_URL,
    VOICE_MODEL_VAR, VOICE_URL_VAR,
};
pub use loopback::{
    DirectRuntime, LoopbackError, LoopbackResolver, LoopbackUrl, SpeechResolver, SpeechUrl,
    SpeechWording, VoiceWording,
};
pub use setup::{
    BootstrapOutcome, BootstrapProgress, BootstrapStage, SetupBackend, SetupError, SetupProgress,
    SetupStage, VoiceSetup, QWEN_ASR_PACKAGE, QWEN_ASR_VLLM_PACKAGE, VOICE_AUTOSTART_VAR,
    VOICE_PYTHON_VAR, VOICE_SETUP_DIR_VAR,
};
pub use speech::{
    chunk_for_speech, OllamaSpeech, SpeechAudio, SpeechError, SpeechStatus, DEFAULT_SPEECH_MODEL,
    DEFAULT_SPEECH_URL, DEFAULT_SPEECH_VOICE, SPEECH_CHAR_LIMIT, SPEECH_MODEL_VAR, SPEECH_URL_VAR,
    SPEECH_VOICE_VAR,
};
