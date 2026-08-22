//! Turning text into a vector, on this machine and nowhere else.
//!
//! The vector has to come from a model, and a model is either a process on
//! this machine or a service somewhere else. There is no third option, and
//! the second one is not available here, so this talks to a local server
//! over loopback HTTP.
//!
//! Ollama is the first and only one, because the settings panel already
//! offers it as the local provider and `zorp-agent` already has a test that
//! drives it. The wire shape is the whole integration: a POST with a model
//! name and a string, an array of floats back.
//!
//! There is no remote provider behind a flag and no fallback when the local
//! one is missing. A capability that quietly works by sending the user's
//! chat history to an API is worse than one that says it is unavailable,
//! and a fallback is the shape that failure takes.

use crate::loopback::{LoopbackError, LoopbackResolver, LoopbackUrl};
use std::fmt;
use std::time::Duration;

/// Where the local embedding server is. Ollama's default port.
pub const DEFAULT_EMBED_URL: &str = "http://127.0.0.1:11434";

/// The model asked for when nothing says otherwise. Small, fast on a CPU,
/// and made for this job rather than for chat.
pub const DEFAULT_EMBED_MODEL: &str = "nomic-embed-text";

/// Override the endpoint. Still checked: naming a remote host here does not
/// get you a remote embedder, it gets you a refusal.
pub const EMBED_URL_VAR: &str = "ZORP_EMBED_URL";

/// Override the model.
pub const EMBED_MODEL_VAR: &str = "ZORP_EMBED_MODEL";

/// Long enough for a cold model to load on a laptop CPU. The connect
/// timeout is short on purpose: a loopback port either has something behind
/// it or it does not, and thirty seconds of waiting to learn that is thirty
/// seconds of a browser spinner saying nothing.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(2);
const READ_TIMEOUT: Duration = Duration::from_secs(120);

/// Something that turns text into a vector. A trait so the tests can supply
/// a deterministic stub, and so a second local runtime is a new type rather
/// than a branch in here.
pub trait Embedder {
    /// A stable name for the model behind this, recorded with the index.
    /// Vectors from two models are not comparable, so the index uses this
    /// to know when everything it holds has become meaningless.
    fn identity(&self) -> String;

    /// One vector for one string. An `Err` is an `Err`: no variant of this
    /// returns an empty vector to mean failure, because an empty vector
    /// stored as a row is a search result waiting to be wrong.
    fn embed(&self, text: &str) -> Result<Vec<f32>, EmbedError>;
}

/// Why no vector was produced.
#[non_exhaustive]
#[derive(Debug)]
pub enum EmbedError {
    /// The configured endpoint is not on this machine. No request was made.
    OffDevice(LoopbackError),
    /// Nothing answered on the loopback endpoint.
    Unreachable { url: String, message: String },
    /// It answered, with an error.
    Status { status: u16, body: String },
    /// It answered with a redirect, which is refused rather than followed.
    Redirected { location: String },
    /// It answered, but not with an embedding.
    Malformed { message: String },
}

impl fmt::Display for EmbedError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            EmbedError::OffDevice(e) => write!(f, "{e}"),
            EmbedError::Unreachable { url, message } => write!(
                f,
                "no local embedder answered at {url} ({message}). \
                 Start one, for example `ollama serve` with `ollama pull {DEFAULT_EMBED_MODEL}`. \
                 Nothing is sent anywhere else"
            ),
            EmbedError::Status { status, body } => {
                write!(f, "the local embedder answered {status}: {body}")
            }
            EmbedError::Redirected { location } => write!(
                f,
                "the local embedder tried to redirect to {location}; \
                 refusing to follow a redirect off this machine"
            ),
            EmbedError::Malformed { message } => {
                write!(
                    f,
                    "the local embedder's answer was not an embedding: {message}"
                )
            }
        }
    }
}

impl std::error::Error for EmbedError {}

impl From<LoopbackError> for EmbedError {
    fn from(e: LoopbackError) -> EmbedError {
        EmbedError::OffDevice(e)
    }
}

/// Ollama, over loopback.
pub struct OllamaEmbedder {
    url: LoopbackUrl,
    model: String,
    agent: ureq::Agent,
}

/// Written out rather than derived, so the HTTP agent's whole configuration
/// does not end up in a log line or a panic message.
impl fmt::Debug for OllamaEmbedder {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("OllamaEmbedder")
            .field("url", &self.url.as_str())
            .field("model", &self.model)
            .finish()
    }
}

impl OllamaEmbedder {
    /// Build one for an endpoint that has already passed the guard.
    pub fn new(url: LoopbackUrl, model: impl Into<String>) -> OllamaEmbedder {
        // Three settings, each closing a way the text could leave.
        //
        // `redirects(0)`: a 302 is a request to send the same body somewhere
        // else, and "somewhere else" is chosen by whatever answered.
        //
        // `try_proxy_from_env(false)`: `AgentBuilder::new` turns this on
        // when the `proxy-from-env` feature is enabled, and Cargo unifies
        // features across the whole graph, so another crate can enable it
        // without this one asking. `HTTP_PROXY` is then someone else's
        // server, receiving the conversation.
        //
        // `resolver`: the last one, and the one that holds if the other two
        // are ever wrong. It answers for one host and port and errors for
        // everything else, and every connection ureq makes, proxied or not,
        // goes through it.
        let agent = ureq::AgentBuilder::new()
            .redirects(0)
            .try_proxy_from_env(false)
            .resolver(LoopbackResolver::for_url(&url))
            .timeout_connect(CONNECT_TIMEOUT)
            .timeout_read(READ_TIMEOUT)
            .build();
        OllamaEmbedder {
            url,
            model: model.into(),
            agent,
        }
    }

    /// Check `url` and build one, or say why not.
    pub fn at(url: &str, model: impl Into<String>) -> Result<OllamaEmbedder, EmbedError> {
        Ok(OllamaEmbedder::new(LoopbackUrl::parse(url)?, model))
    }

    /// The endpoint and model from the environment, or the local defaults.
    pub fn from_env() -> Result<OllamaEmbedder, EmbedError> {
        let url = non_empty(EMBED_URL_VAR).unwrap_or_else(|| DEFAULT_EMBED_URL.to_string());
        let model = non_empty(EMBED_MODEL_VAR).unwrap_or_else(|| DEFAULT_EMBED_MODEL.to_string());
        OllamaEmbedder::at(&url, model)
    }

    pub fn endpoint(&self) -> &str {
        self.url.as_str()
    }

    pub fn model(&self) -> &str {
        &self.model
    }
}

impl Embedder for OllamaEmbedder {
    fn identity(&self) -> String {
        format!("ollama/{}", self.model)
    }

    fn embed(&self, text: &str) -> Result<Vec<f32>, EmbedError> {
        let endpoint = format!("{}/api/embeddings", self.url.as_str());
        let response = self
            .agent
            .post(&endpoint)
            // No Authorization header, ever. There is no credential for a
            // local model, and a header habit is how one ends up being sent
            // to whatever the endpoint turns out to be.
            .send_json(serde_json::json!({"model": self.model, "prompt": text}));

        let response = match response {
            Ok(r) => r,
            Err(ureq::Error::Status(status, r)) => {
                let body = r.into_string().unwrap_or_default();
                return Err(EmbedError::Status {
                    status,
                    body: body.chars().take(400).collect(),
                });
            }
            Err(ureq::Error::Transport(t)) => {
                return Err(EmbedError::Unreachable {
                    url: self.url.as_str().to_string(),
                    message: t.to_string(),
                })
            }
        };

        // Redirects are off, so a 3xx arrives here as an ordinary response
        // rather than being followed. That is the whole reason to look at
        // the status of something that is not an error.
        if (300..400).contains(&response.status()) {
            let location = response
                .header("location")
                .unwrap_or("an unnamed location")
                .to_string();
            return Err(EmbedError::Redirected { location });
        }

        let body = response.into_string().map_err(|e| EmbedError::Malformed {
            message: e.to_string(),
        })?;
        parse_embedding(&body)
    }
}

/// Ollama's `/api/embeddings` answers `{"embedding": [...]}`. Its newer
/// `/api/embed` answers `{"embeddings": [[...]]}` and some builds answer
/// that shape from either path, so both are read. Anything else, including
/// an empty vector, is an error.
fn parse_embedding(body: &str) -> Result<Vec<f32>, EmbedError> {
    let value: serde_json::Value =
        serde_json::from_str(body).map_err(|e| EmbedError::Malformed {
            message: e.to_string(),
        })?;
    let array = value
        .get("embedding")
        .or_else(|| value.get("embeddings").and_then(|e| e.get(0)))
        .and_then(|v| v.as_array())
        .ok_or_else(|| EmbedError::Malformed {
            message: "no `embedding` array in the answer".into(),
        })?;
    if array.is_empty() {
        return Err(EmbedError::Malformed {
            message: "the `embedding` array was empty".into(),
        });
    }
    array
        .iter()
        .map(|v| {
            v.as_f64()
                .map(|f| f as f32)
                .ok_or_else(|| EmbedError::Malformed {
                    message: "the `embedding` array holds something that is not a number".into(),
                })
        })
        .collect()
}

fn non_empty(var: &str) -> Option<String> {
    std::env::var(var).ok().filter(|v| !v.trim().is_empty())
}
