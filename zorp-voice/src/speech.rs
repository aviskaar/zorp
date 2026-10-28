//! Text to speech over checked loopback, against a local Ollama server.
//!
//! The ASR half of this crate takes a recording and returns text. This is the
//! other half: it takes text and returns audio, spoken by Ollama's `phonon`
//! model at the OpenAI-compatible `POST /v1/audio/speech`, and lists the
//! voices a speech model offers from `POST /api/show`.
//!
//! One rule holds it up, the same one the whole crate is arranged around: the
//! text goes to a loopback address or it goes nowhere. An answer can quote a
//! person's files, so the text handed to a TTS runtime gets the same boundary
//! as a recording of their voice. There is no cloud TTS provider and no
//! fallback, enforced by the four layers `LoopbackUrl`, `LoopbackResolver`,
//! `redirects(0)` and `try_proxy_from_env(false)` give, and proved by a
//! connection-counting canary test.
//!
//! The TTS model is a selectable setting, not a hardcoded `phonon`. Phonon is
//! the only model today, but others are coming, so `speak` and `voices` take
//! the model by name the way the chat path does.

use crate::loopback::{LoopbackError, SpeechResolver, SpeechUrl};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::io::Read;
use std::time::Duration;

/// The same local Ollama server recall already embeds against.
pub const DEFAULT_SPEECH_URL: &str = "http://127.0.0.1:11434";
pub const SPEECH_URL_VAR: &str = "ZORP_SPEECH_URL";
/// The only TTS model Ollama ships today. Treated as a setting, not baked in.
pub const DEFAULT_SPEECH_MODEL: &str = "phonon";
pub const SPEECH_MODEL_VAR: &str = "ZORP_SPEECH_MODEL";
/// Phonon's own voice, used when none is chosen.
pub const DEFAULT_SPEECH_VOICE: &str = "default";
pub const SPEECH_VOICE_VAR: &str = "ZORP_SPEECH_VOICE";

/// The per-request character ceiling Ollama documents for phonon. Longer text
/// is split on sentence boundaries under this before it is sent, so playback
/// can start on the first chunk.
pub const SPEECH_CHAR_LIMIT: usize = 4096;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(2);
// TTS synthesis of a long chunk can take a while on first load, so this
// follows the ASR read timeout rather than the shorter probe timeouts.
const READ_TIMEOUT: Duration = Duration::from_secs(900);

/// What a speech runtime is doing right now, for the status route.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SpeechStatus {
    pub endpoint: String,
    pub model: String,
    pub runtime_reachable: bool,
    pub model_present: bool,
    /// The voices the model offers, from `/api/show`. Empty when the model is
    /// absent or the runtime named none.
    pub voices: Vec<String>,
    pub detail: String,
}

/// Audio bytes and the media type the runtime labelled them with, passed
/// straight through to the browser so it can play them without guessing.
#[derive(Debug, Clone)]
pub struct SpeechAudio {
    pub bytes: Vec<u8>,
    pub media_type: String,
}

#[non_exhaustive]
#[derive(Debug)]
pub enum SpeechError {
    OffDevice(LoopbackError),
    Unreachable { url: String, message: String },
    Status { status: u16, body: String },
    Redirected { location: String },
    Empty,
    Malformed { message: String },
}

impl fmt::Display for SpeechError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SpeechError::OffDevice(error) => write!(f, "{error}"),
            SpeechError::Unreachable { url, message } => write!(
                f,
                "no local Ollama speech runtime answered at {url} ({message}). Start Ollama 0.40.9 or later and pull the phonon model. Nothing is sent anywhere else"
            ),
            SpeechError::Status { status, body } => {
                write!(f, "the local speech runtime answered {status}: {body}")
            }
            SpeechError::Redirected { location } => write!(
                f,
                "the local speech runtime tried to redirect to {location}; refusing to follow it"
            ),
            SpeechError::Empty => write!(f, "there was no text to speak"),
            SpeechError::Malformed { message } => {
                write!(f, "the local speech runtime returned an invalid answer: {message}")
            }
        }
    }
}

impl std::error::Error for SpeechError {}

impl From<LoopbackError> for SpeechError {
    fn from(error: LoopbackError) -> Self {
        SpeechError::OffDevice(error)
    }
}

/// A client for a local Ollama server's speech endpoints.
pub struct OllamaSpeech {
    url: SpeechUrl,
    model: String,
    agent: ureq::Agent,
}

impl fmt::Debug for OllamaSpeech {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("OllamaSpeech")
            .field("url", &self.url.as_str())
            .field("model", &self.model)
            .finish()
    }
}

impl OllamaSpeech {
    pub fn new(url: SpeechUrl, model: impl Into<String>) -> OllamaSpeech {
        let agent = ureq::AgentBuilder::new()
            .redirects(0)
            .try_proxy_from_env(false)
            .resolver(SpeechResolver::for_url(&url))
            .timeout_connect(CONNECT_TIMEOUT)
            .timeout_read(READ_TIMEOUT)
            .build();
        OllamaSpeech {
            url,
            model: model.into(),
            agent,
        }
    }

    pub fn at(url: &str, model: impl Into<String>) -> Result<OllamaSpeech, SpeechError> {
        Ok(OllamaSpeech::new(SpeechUrl::parse(url)?, model))
    }

    pub fn from_env() -> Result<OllamaSpeech, SpeechError> {
        let url = non_empty(SPEECH_URL_VAR).unwrap_or_else(|| DEFAULT_SPEECH_URL.to_string());
        let model = non_empty(SPEECH_MODEL_VAR).unwrap_or_else(|| DEFAULT_SPEECH_MODEL.to_string());
        OllamaSpeech::at(&url, model)
    }

    pub fn endpoint(&self) -> &str {
        self.url.as_str()
    }

    /// The base URL with any single trailing slash removed, so joining a path
    /// that starts with `/` does not produce a `//`. Ollama tolerates the
    /// double slash, but a configured `ZORP_SPEECH_URL` ending in `/` should
    /// not change the request we make.
    fn base(&self) -> &str {
        self.url.as_str().trim_end_matches('/')
    }

    pub fn model(&self) -> &str {
        &self.model
    }

    /// Reachability, whether the model is present, and the voices it offers.
    pub fn status(&self) -> SpeechStatus {
        let mut status = SpeechStatus {
            endpoint: self.endpoint().to_string(),
            model: self.model.clone(),
            runtime_reachable: false,
            model_present: false,
            voices: Vec::new(),
            detail: String::new(),
        };
        // `/api/tags` is the cheapest thing Ollama answers and proves it is
        // the Ollama server rather than some other process on the port.
        let tags = match self.get_text("/api/tags") {
            Ok(body) => body,
            Err(SpeechError::Status { .. }) => {
                // It answered, just not 200 for tags: still reachable.
                status.runtime_reachable = true;
                status.detail = "the local Ollama server is running".into();
                return status;
            }
            Err(_) => {
                status.detail =
                    "no local Ollama server is running. Start Ollama and pull the phonon model"
                        .into();
                return status;
            }
        };
        status.runtime_reachable = true;
        status.model_present = model_in_tags(&tags, &self.model);
        if !status.model_present {
            status.detail = format!(
                "the local Ollama server is running, but the {} model is not pulled",
                self.model
            );
            return status;
        }
        match self.voices(&self.model) {
            Ok(voices) => {
                status.voices = voices;
                status.detail = "the local speech runtime and model are ready".into();
            }
            Err(error) => status.detail = error.to_string(),
        }
        status
    }

    /// The voices `model` offers, from `POST /api/show`.
    pub fn voices(&self, model: &str) -> Result<Vec<String>, SpeechError> {
        let body = self.post_text("/api/show", serde_json::json!({ "model": model }))?;
        let value: serde_json::Value =
            serde_json::from_str(&body).map_err(|error| SpeechError::Malformed {
                message: error.to_string(),
            })?;
        Ok(extract_voices(&value))
    }

    /// Synthesize `text` with `model` and `voice`. Caller chunks first.
    pub fn speak(&self, text: &str, model: &str, voice: &str) -> Result<SpeechAudio, SpeechError> {
        if text.trim().is_empty() {
            return Err(SpeechError::Empty);
        }
        let body = serde_json::json!({
            "model": model,
            "input": text,
            "voice": voice,
        });
        let endpoint = format!("{}/v1/audio/speech", self.base());
        let response = request_result(self.agent.post(&endpoint).send_json(body), &self.url)?;
        let media_type = response
            .header("content-type")
            .unwrap_or("audio/mpeg")
            .to_string();
        let mut bytes = Vec::new();
        response
            .into_reader()
            .take(64 * 1024 * 1024)
            .read_to_end(&mut bytes)
            .map_err(|error| SpeechError::Malformed {
                message: error.to_string(),
            })?;
        if bytes.is_empty() {
            return Err(SpeechError::Malformed {
                message: "the speech runtime returned no audio".into(),
            });
        }
        Ok(SpeechAudio { bytes, media_type })
    }

    fn get_text(&self, path: &str) -> Result<String, SpeechError> {
        let endpoint = format!("{}{path}", self.base());
        request_result(self.agent.get(&endpoint).call(), &self.url)?
            .into_string()
            .map_err(|error| SpeechError::Malformed {
                message: error.to_string(),
            })
    }

    fn post_text(&self, path: &str, body: serde_json::Value) -> Result<String, SpeechError> {
        let endpoint = format!("{}{path}", self.base());
        request_result(self.agent.post(&endpoint).send_json(body), &self.url)?
            .into_string()
            .map_err(|error| SpeechError::Malformed {
                message: error.to_string(),
            })
    }
}

fn request_result(
    result: Result<ureq::Response, ureq::Error>,
    url: &SpeechUrl,
) -> Result<ureq::Response, SpeechError> {
    let response = match result {
        Ok(response) => response,
        Err(ureq::Error::Status(status, response)) => {
            return Err(SpeechError::Status {
                status,
                body: response
                    .into_string()
                    .unwrap_or_default()
                    .chars()
                    .take(400)
                    .collect(),
            });
        }
        Err(ureq::Error::Transport(error)) => {
            return Err(SpeechError::Unreachable {
                url: url.as_str().into(),
                message: error.to_string(),
            });
        }
    };
    if (300..400).contains(&response.status()) {
        return Err(SpeechError::Redirected {
            location: response
                .header("location")
                .unwrap_or("an unnamed location")
                .to_string(),
        });
    }
    Ok(response)
}

/// Whether `model` is in an `/api/tags` listing, treating a bare name and a
/// `name:tag` the same when the configured model named no tag.
fn model_in_tags(body: &str, model: &str) -> bool {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(body) else {
        return false;
    };
    let Some(models) = value.get("models").and_then(serde_json::Value::as_array) else {
        return false;
    };
    models.iter().any(|entry| {
        entry
            .get("name")
            .and_then(serde_json::Value::as_str)
            .is_some_and(|name| name == model || name.split(':').next() == Some(model))
    })
}

/// Pull the voice list out of an `/api/show` answer.
///
/// The exact field name for a speech model's voices is not pinned here on
/// purpose: it was not possible to confirm it against Ollama's docs when this
/// was written, and a model other than phonon may word it differently. So
/// this searches the answer for the first array of strings under any key that
/// is `voices` or ends in `.voices` (Ollama nests model metadata under keys
/// like `phonon.voices` inside `model_info`), and falls back to a top-level
/// `voices`. An answer with none yields an empty list, and the caller offers
/// the default voice rather than failing. This is the one place to adjust
/// once the field name is confirmed.
fn extract_voices(value: &serde_json::Value) -> Vec<String> {
    fn string_array(value: &serde_json::Value) -> Option<Vec<String>> {
        let array = value.as_array()?;
        let strings: Vec<String> = array
            .iter()
            .filter_map(|v| v.as_str().map(str::to_string))
            .collect();
        (!strings.is_empty() && strings.len() == array.len()).then_some(strings)
    }
    fn search(value: &serde_json::Value, out: &mut Vec<String>) {
        if let Some(map) = value.as_object() {
            for (key, child) in map {
                let is_voices_key = key == "voices" || key.ends_with(".voices");
                if is_voices_key {
                    if let Some(found) = string_array(child) {
                        for voice in found {
                            if !out.contains(&voice) {
                                out.push(voice);
                            }
                        }
                        continue;
                    }
                }
                search(child, out);
            }
        }
    }
    let mut out = Vec::new();
    search(value, &mut out);
    out
}

/// Split `text` into pieces no longer than `limit` characters, cut at
/// sentence boundaries where it can and at a whitespace boundary (then a hard
/// character boundary) for a sentence that is itself too long. Every piece is
/// non-empty and the pieces in order rejoin to the input's content.
pub fn chunk_for_speech(text: &str, limit: usize) -> Vec<String> {
    let limit = limit.max(1);
    let mut chunks = Vec::new();
    let mut current = String::new();
    for sentence in sentences(text) {
        if sentence.chars().count() > limit {
            if !current.is_empty() {
                chunks.push(std::mem::take(&mut current));
            }
            for piece in hard_split(sentence, limit) {
                chunks.push(piece);
            }
            continue;
        }
        if current.chars().count() + sentence.chars().count() > limit && !current.is_empty() {
            chunks.push(std::mem::take(&mut current));
        }
        current.push_str(sentence);
    }
    if !current.trim().is_empty() {
        chunks.push(current);
    }
    chunks
        .into_iter()
        .map(|c| c.trim().to_string())
        .filter(|c| !c.is_empty())
        .collect()
}

/// Sentence-ish slices, keeping the terminator and the trailing space with
/// the sentence so rejoining is lossless. Splits after `.`, `!`, `?` and a
/// newline.
fn sentences(text: &str) -> Vec<&str> {
    let bytes = text.as_bytes();
    let mut out = Vec::new();
    let mut start = 0;
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        let boundary = matches!(b, b'.' | b'!' | b'?' | b'\n');
        // A terminator is a boundary only when the next byte is whitespace or
        // the end, so a decimal point or a `v1.2` does not split a sentence.
        let next_is_space = bytes.get(i + 1).is_none_or(|n| n.is_ascii_whitespace());
        if boundary && (b == b'\n' || next_is_space) {
            out.push(&text[start..=i]);
            start = i + 1;
        }
        i += 1;
    }
    if start < text.len() {
        out.push(&text[start..]);
    }
    out
}

/// Break an over-long sentence at whitespace under `limit`, and at a
/// character boundary when a single run of non-space is itself too long.
fn hard_split(text: &str, limit: usize) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = String::new();
    for word in text.split_inclusive(char::is_whitespace) {
        if word.chars().count() > limit {
            if !current.is_empty() {
                out.push(std::mem::take(&mut current));
            }
            let mut piece = String::new();
            for ch in word.chars() {
                if piece.chars().count() + 1 > limit {
                    out.push(std::mem::take(&mut piece));
                }
                piece.push(ch);
            }
            current = piece;
            continue;
        }
        if current.chars().count() + word.chars().count() > limit && !current.is_empty() {
            out.push(std::mem::take(&mut current));
        }
        current.push_str(word);
    }
    if !current.is_empty() {
        out.push(current);
    }
    out.into_iter()
        .map(|c| c.trim().to_string())
        .filter(|c| !c.is_empty())
        .collect()
}

fn non_empty(var: &str) -> Option<String> {
    std::env::var(var)
        .ok()
        .filter(|value| !value.trim().is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_trailing_slash_on_the_base_does_not_double_the_path_separator() {
        let with = OllamaSpeech::at("http://127.0.0.1:11434/", "phonon").unwrap();
        assert_eq!(with.base(), "http://127.0.0.1:11434");
        let without = OllamaSpeech::at("http://127.0.0.1:11434", "phonon").unwrap();
        assert_eq!(without.base(), "http://127.0.0.1:11434");
    }

    #[test]
    fn model_presence_matches_bare_and_tagged_names() {
        let body = r#"{"models":[{"name":"phonon:latest"},{"name":"llama3"}]}"#;
        assert!(model_in_tags(body, "phonon"));
        assert!(model_in_tags(body, "llama3"));
        assert!(!model_in_tags(body, "mistral"));
    }

    #[test]
    fn voices_are_found_under_a_nested_voices_key() {
        let value = serde_json::json!({
            "model_info": { "phonon.voices": ["default", "river", "stone"] },
            "details": { "family": "phonon" }
        });
        assert_eq!(extract_voices(&value), vec!["default", "river", "stone"]);
    }

    #[test]
    fn voices_are_found_under_a_top_level_key_and_deduped() {
        let value = serde_json::json!({ "voices": ["a", "b", "a"] });
        assert_eq!(extract_voices(&value), vec!["a", "b"]);
    }

    #[test]
    fn no_voices_yields_an_empty_list_rather_than_an_error() {
        let value = serde_json::json!({ "details": { "family": "phonon" } });
        assert!(extract_voices(&value).is_empty());
        // A `voices` key that is not an array of strings is ignored, not
        // half-read.
        let mixed = serde_json::json!({ "voices": ["a", 7] });
        assert!(extract_voices(&mixed).is_empty());
    }

    #[test]
    fn short_text_is_one_chunk() {
        let chunks = chunk_for_speech("Hello there. How are you?", 4096);
        assert_eq!(chunks, vec!["Hello there. How are you?"]);
    }

    #[test]
    fn long_text_splits_on_sentence_boundaries_under_the_limit() {
        let text = "One sentence here. Two sentence here. Three sentence here.";
        let chunks = chunk_for_speech(text, 20);
        assert!(chunks.iter().all(|c| c.chars().count() <= 20), "{chunks:?}");
        // Nothing is dropped: the words all survive in order.
        let rejoined = chunks.join(" ");
        for word in ["One", "Two", "Three", "sentence"] {
            assert!(rejoined.contains(word), "{rejoined}");
        }
        // It really split.
        assert!(chunks.len() >= 3, "{chunks:?}");
    }

    #[test]
    fn a_sentence_longer_than_the_limit_is_hard_split() {
        let text = "wordwordword ".repeat(10);
        let chunks = chunk_for_speech(&text, 12);
        assert!(!chunks.is_empty());
        assert!(chunks.iter().all(|c| c.chars().count() <= 12), "{chunks:?}");
    }

    #[test]
    fn a_decimal_point_does_not_split_a_sentence() {
        let chunks = chunk_for_speech("Version 1.2 is out now.", 4096);
        assert_eq!(chunks, vec!["Version 1.2 is out now."]);
    }

    #[test]
    fn empty_text_speaks_nothing() {
        assert!(chunk_for_speech("   ", 4096).is_empty());
    }
}
