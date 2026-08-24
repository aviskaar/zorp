//! The Qwen ASR 0.0.6 voice runtime, over checked loopback HTTP.

use crate::loopback::{LoopbackError, LoopbackResolver, LoopbackUrl};
use base64::Engine as _;
use serde::{Deserialize, Serialize};
use std::fmt;
use std::time::Duration;

pub const DEFAULT_VOICE_URL: &str = "http://127.0.0.1:8000";
pub const DEFAULT_VOICE_MODEL: &str = "Qwen/Qwen3-ASR-0.6B";
pub const VOICE_URL_VAR: &str = "ZORP_VOICE_URL";
pub const VOICE_MODEL_VAR: &str = "ZORP_VOICE_MODEL";

const CONNECT_TIMEOUT: Duration = Duration::from_secs(2);
const READ_TIMEOUT: Duration = Duration::from_secs(900);

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct VoiceStatus {
    pub endpoint: String,
    pub model: String,
    pub runtime_reachable: bool,
    pub model_present: bool,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Transcription {
    pub text: String,
    pub language: String,
}

#[non_exhaustive]
#[derive(Debug)]
pub enum VoiceError {
    OffDevice(LoopbackError),
    Unreachable { url: String, message: String },
    Status { status: u16, body: String },
    Redirected { location: String },
    UnsupportedMedia { media_type: String },
    Malformed { message: String },
}

impl fmt::Display for VoiceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            VoiceError::OffDevice(error) => write!(f, "{error}"),
            VoiceError::Unreachable { url, message } => write!(
                f,
                "no local voice runtime answered at {url} ({message}). Start `qwen-asr-serve`. Nothing is sent anywhere else"
            ),
            VoiceError::Status { status, body } => {
                write!(f, "the local voice runtime answered {status}: {body}")
            }
            VoiceError::Redirected { location } => write!(
                f,
                "the local voice runtime tried to redirect to {location}; refusing to follow it"
            ),
            VoiceError::UnsupportedMedia { media_type } => {
                write!(f, "{media_type:?} is not a supported browser audio type")
            }
            VoiceError::Malformed { message } => {
                write!(f, "the local voice runtime returned an invalid answer: {message}")
            }
        }
    }
}

impl std::error::Error for VoiceError {}

impl From<LoopbackError> for VoiceError {
    fn from(error: LoopbackError) -> Self {
        VoiceError::OffDevice(error)
    }
}

/// A client for the OpenAI-compatible endpoint started by `qwen-asr-serve`.
pub struct QwenAsr {
    url: LoopbackUrl,
    model: String,
    agent: ureq::Agent,
}

impl fmt::Debug for QwenAsr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("QwenAsr")
            .field("url", &self.url.as_str())
            .field("model", &self.model)
            .finish()
    }
}

impl QwenAsr {
    pub fn new(url: LoopbackUrl, model: impl Into<String>) -> QwenAsr {
        let agent = ureq::AgentBuilder::new()
            .redirects(0)
            .try_proxy_from_env(false)
            .resolver(LoopbackResolver::for_url(&url))
            .timeout_connect(CONNECT_TIMEOUT)
            .timeout_read(READ_TIMEOUT)
            .build();
        QwenAsr {
            url,
            model: model.into(),
            agent,
        }
    }

    pub fn at(url: &str, model: impl Into<String>) -> Result<QwenAsr, VoiceError> {
        Ok(QwenAsr::new(LoopbackUrl::parse(url)?, model))
    }

    pub fn from_env() -> Result<QwenAsr, VoiceError> {
        let url = non_empty(VOICE_URL_VAR).unwrap_or_else(|| DEFAULT_VOICE_URL.to_string());
        let model = non_empty(VOICE_MODEL_VAR).unwrap_or_else(|| DEFAULT_VOICE_MODEL.to_string());
        QwenAsr::at(&url, model)
    }

    pub fn endpoint(&self) -> &str {
        self.url.as_str()
    }

    pub fn model(&self) -> &str {
        &self.model
    }

    /// A copyable operator command for a direct HTTP endpoint.
    ///
    /// It is shown as text and never executed by zorp. HTTPS and path-prefixed
    /// endpoints need an operator-managed loopback proxy, so zorp cannot
    /// construct the full command for them.
    pub fn start_command(&self) -> Option<String> {
        self.url.supports_direct_runtime().then(|| format!(
            "python -m pip install \"qwen-asr[vllm]==0.0.6\"\nqwen-asr-serve {} --host {} --port {}",
            shell_word(&self.model),
            shell_word(self.url.host()),
            self.url.port()
        ))
    }

    pub fn status(&self) -> VoiceStatus {
        let mut status = VoiceStatus {
            endpoint: self.endpoint().to_string(),
            model: self.model.clone(),
            runtime_reachable: false,
            model_present: false,
            detail: String::new(),
        };
        if let Err(error) = self.get_text("/health") {
            status.detail = self.with_operator_guidance(error.to_string());
            return status;
        }
        status.runtime_reachable = true;
        let models = match self.get_text("/v1/models") {
            Ok(body) => body,
            Err(error) => {
                status.detail = self.with_operator_guidance(error.to_string());
                return status;
            }
        };
        match model_is_present(&models, &self.model) {
            Ok(present) => {
                status.model_present = present;
                status.detail = if present {
                    "the local Qwen3-ASR runtime and model are ready".into()
                } else {
                    "the local runtime is ready, but it is not serving the configured Qwen3-ASR model"
                        .into()
                };
                status.detail = self.with_operator_guidance(status.detail);
            }
            Err(error) => status.detail = self.with_operator_guidance(error.to_string()),
        }
        status
    }

    pub fn transcribe(&self, audio: &[u8], media_type: &str) -> Result<Transcription, VoiceError> {
        if audio.is_empty() {
            return Err(VoiceError::Malformed {
                message: "the recording was empty".into(),
            });
        }
        let body = chat_body(&self.model, audio, media_type)?;

        let endpoint = format!("{}/v1/chat/completions", self.url.as_str());
        let response = request_result(self.agent.post(&endpoint).send_json(body), &self.url)?;
        let body = response
            .into_string()
            .map_err(|error| VoiceError::Malformed {
                message: error.to_string(),
            })?;
        parse_transcription(&body)
    }

    fn get_text(&self, path: &str) -> Result<String, VoiceError> {
        let endpoint = format!("{}{path}", self.url.as_str());
        request_result(self.agent.get(&endpoint).call(), &self.url)?
            .into_string()
            .map_err(|error| VoiceError::Malformed {
                message: error.to_string(),
            })
    }

    fn with_operator_guidance(&self, detail: String) -> String {
        if self.url.supports_direct_runtime() {
            detail
        } else {
            format!(
                "{detail}. This configured HTTPS or path endpoint needs an operator-managed loopback proxy. Start `qwen-asr-serve` behind that proxy"
            )
        }
    }
}

fn request_result(
    result: Result<ureq::Response, ureq::Error>,
    url: &LoopbackUrl,
) -> Result<ureq::Response, VoiceError> {
    let response = match result {
        Ok(response) => response,
        Err(ureq::Error::Status(status, response)) => {
            return Err(VoiceError::Status {
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
            return Err(VoiceError::Unreachable {
                url: url.as_str().into(),
                message: error.to_string(),
            });
        }
    };
    if (300..400).contains(&response.status()) {
        return Err(VoiceError::Redirected {
            location: response
                .header("location")
                .unwrap_or("an unnamed location")
                .to_string(),
        });
    }
    Ok(response)
}

fn supported_media_type(media_type: &str) -> Result<&str, VoiceError> {
    let mime = media_type.split(';').next().unwrap_or_default().trim();
    match mime {
        "audio/webm" | "audio/ogg" | "audio/mp4" | "audio/mpeg" | "audio/wav" | "audio/x-wav" => {
            Ok(mime)
        }
        _ => Err(VoiceError::UnsupportedMedia {
            media_type: media_type.into(),
        }),
    }
}

fn chat_body(model: &str, audio: &[u8], media_type: &str) -> Result<serde_json::Value, VoiceError> {
    let mime = supported_media_type(media_type)?;
    let encoded = base64::engine::general_purpose::STANDARD.encode(audio);
    let data_url = format!("data:{mime};base64,{encoded}");
    Ok(serde_json::json!({
        "model": model,
        "messages": [{
            "role": "user",
            "content": [{
                "type": "audio_url",
                "audio_url": {"url": data_url}
            }]
        }]
    }))
}

fn model_is_present(body: &str, model: &str) -> Result<bool, VoiceError> {
    let value: serde_json::Value =
        serde_json::from_str(body).map_err(|error| VoiceError::Malformed {
            message: format!("invalid model list: {error}"),
        })?;
    let models = value
        .get("data")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| VoiceError::Malformed {
            message: "the model list has no `data` array".into(),
        })?;
    let wanted = canonical_model(model);
    Ok(models.iter().any(|entry| {
        entry
            .get("id")
            .and_then(serde_json::Value::as_str)
            .is_some_and(|id| canonical_model(id) == wanted)
    }))
}

fn canonical_model(model: &str) -> &str {
    model.strip_suffix("@main").unwrap_or(model)
}

fn parse_transcription(body: &str) -> Result<Transcription, VoiceError> {
    let value: serde_json::Value =
        serde_json::from_str(body).map_err(|error| VoiceError::Malformed {
            message: error.to_string(),
        })?;
    let raw = value
        .get("choices")
        .and_then(serde_json::Value::as_array)
        .and_then(|choices| choices.first())
        .and_then(|choice| choice.get("message"))
        .and_then(|message| message.get("content"))
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| VoiceError::Malformed {
            message: "the chat completion has no message content".into(),
        })?;
    let (metadata, text) =
        raw.trim()
            .split_once("<asr_text>")
            .ok_or_else(|| VoiceError::Malformed {
                message: "Qwen3-ASR did not separate its language tag from the transcript".into(),
            })?;
    let language = metadata
        .lines()
        .find_map(|line| {
            let (key, value) = line.trim().split_once(' ')?;
            key.eq_ignore_ascii_case("language").then(|| value.trim())
        })
        .filter(|language| !language.is_empty() && !language.eq_ignore_ascii_case("none"))
        .ok_or_else(|| VoiceError::Malformed {
            message: "Qwen3-ASR did not return a detected language tag".into(),
        })?;
    if text.trim().is_empty() {
        return Err(VoiceError::Malformed {
            message: "Qwen3-ASR returned an empty transcript".into(),
        });
    }
    Ok(Transcription {
        text: text.trim().into(),
        language: language.trim().into(),
    })
}

fn shell_word(value: &str) -> String {
    if !value.is_empty()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"/_.:-".contains(&byte))
    {
        return value.to_string();
    }
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn non_empty(var: &str) -> Option<String> {
    std::env::var(var)
        .ok()
        .filter(|value| !value.trim().is_empty())
}

#[cfg(test)]
mod tests {
    use super::{chat_body, model_is_present, parse_transcription, shell_word, QwenAsr};

    #[test]
    fn qwen_language_envelope_is_split_without_translation() {
        let value = parse_transcription(
            r#"{"choices":[{"message":{"content":"language 粤语<asr_text>早晨，今日天氣幾好。"}}]}"#,
        )
        .unwrap();
        assert_eq!(value.language, "粤语");
        assert_eq!(value.text, "早晨，今日天氣幾好。");
    }

    #[test]
    fn qwen_multiline_metadata_uses_only_the_language_line() {
        let value = parse_transcription(
            r#"{"choices":[{"message":{"content":"language Chinese\nmetadata zorp does not interpret\n<asr_text>hello"}}]}"#,
        )
        .unwrap();
        assert_eq!(value.language, "Chinese");
        assert_eq!(value.text, "hello");
    }

    #[test]
    fn silence_and_untagged_output_do_not_invent_a_language() {
        for body in [
            r#"{"choices":[{"message":{"content":"language None<asr_text>"}}]}"#,
            r#"{"choices":[{"message":{"content":"plain text without a language tag"}}]}"#,
        ] {
            let error = parse_transcription(body).unwrap_err();
            assert!(error.to_string().contains("language"), "{error}");
        }
    }

    #[test]
    fn loaded_model_ids_match_the_configured_model() {
        let body = r#"{"data":[{"id":"Qwen/Qwen3-ASR-0.6B"}]}"#;
        assert!(model_is_present(body, "Qwen/Qwen3-ASR-0.6B").unwrap());
    }

    #[test]
    fn chat_body_carries_audio_without_a_language_prompt() {
        let value = chat_body("Qwen/model", b"audio", "audio/webm;codecs=opus").unwrap();
        assert_eq!(value["model"], "Qwen/model");
        assert_eq!(value["messages"][0]["role"], "user");
        assert_eq!(value["messages"][0]["content"][0]["type"], "audio_url");
        assert_eq!(
            value["messages"][0]["content"][0]["audio_url"]["url"],
            "data:audio/webm;base64,YXVkaW8="
        );
        assert!(!value.to_string().contains("English"));
    }

    #[test]
    fn operator_commands_quote_model_names() {
        assert_eq!(shell_word("Qwen/model"), "Qwen/model");
        assert_eq!(shell_word("model; touch /tmp/no"), "'model; touch /tmp/no'");
        assert_eq!(shell_word("a'b"), "'a'\\''b'");
        let client = QwenAsr::at("http://127.0.0.1:8123", "Qwen/model").unwrap();
        assert_eq!(
            client.start_command().as_deref(),
            Some(
            "python -m pip install \"qwen-asr[vllm]==0.0.6\"\nqwen-asr-serve Qwen/model --host 127.0.0.1 --port 8123"
            )
        );
        assert!(QwenAsr::at("https://127.0.0.1:8123", "Qwen/model")
            .unwrap()
            .start_command()
            .is_none());
        assert!(QwenAsr::at("http://127.0.0.1:8123/proxy", "Qwen/model")
            .unwrap()
            .start_command()
            .is_none());
    }
}
