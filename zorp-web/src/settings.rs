//! Model settings: resolving a UI-saved value over the matching `ZORP_*` env
//! var over a hardcoded default, and persisting the non-secret fields to
//! `~/.config/zorp/web.toml`. See `docs/DECISIONS.md` (2026-08-17).
//!
//! Before this existed, `zorp-web` learned what model to talk to only from
//! `HttpModel::try_from_env`, which silently defaults to
//! `https://api.openai.com/v1` and `gpt-4o` with no API key when nothing is
//! set. A user with no `ZORP_*` vars got a UI that loaded fine and then died
//! on the first message, deep inside the provider call. This module is what
//! lets the chat UI's settings panel take over instead, and what lets the
//! server say "not configured" before that first message is ever sent.
//!
//! The API key is never written to disk: `PersistedSettings`, the only shape
//! serialized to the config file, has no field that could carry it. It lives
//! in `SettingsState::api_key` for the life of the server process, seeded
//! once from `ZORP_API_KEY` at startup (`SettingsState::seeded_from_env`)
//! and replaced in memory by a UI save. It is also never sent back out over
//! HTTP: `Resolved`, the shape `GET /api/settings` answers with, carries
//! `has_api_key: bool` and nothing else that could leak it.

use serde::{Deserialize, Serialize};
use std::io;
use std::path::PathBuf;
use zorp_agent::Provider;

/// Hardcoded fallback base URL. Unchanged from `HttpModel::from_env`'s
/// default so a server with nothing configured anywhere behaves exactly as
/// it did before this module existed.
pub const DEFAULT_BASE_URL: &str = "https://api.openai.com/v1";
/// Hardcoded fallback model, same reasoning.
pub const DEFAULT_MODEL: &str = "gpt-4o";

/// What to name the model in a transcription upload when nothing else is
/// chosen. whisper.cpp ignores the field entirely and serves whatever model
/// it was started with; OpenAI-compatible runtimes that host several models
/// use it to pick one. `whisper-1` is what every one of them recognises.
pub const DEFAULT_TRANSCRIBE_MODEL: &str = "whisper-1";

/// Env var that overrides where the settings file lives. Real usage always
/// resolves to `~/.config/zorp/web.toml`; this exists so tests can point it
/// at a private temp file instead of touching the developer's real config.
const CONFIG_PATH_VAR: &str = "ZORP_WEB_CONFIG";

/// Where an effective field's value came from, so the UI can say "from
/// ZORP_MODEL" instead of implying the user chose it.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Source {
    Ui,
    Env,
    Default,
}

fn provider_str(p: Provider) -> &'static str {
    match p {
        Provider::OpenAiCompatible => "openai",
        Provider::Anthropic => "anthropic",
    }
}

/// The only shape ever written to the settings file. No `api_key` field
/// exists here on purpose: there is nothing on this struct to accidentally
/// serialize a secret through.
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
pub struct PersistedSettings {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,
    /// Where recorded speech is sent to be turned into text. Persisted like
    /// the other endpoints, and secret-free for the same reason: zorp never
    /// authenticates to it, so there is no key to keep out of this file.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub transcribe_base_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub transcribe_model: Option<String>,
}

/// Body of `PUT /api/settings`. Every field is optional: a PUT only changes
/// the fields it names, leaving the rest of the stored state alone.
/// `provider` is a raw string rather than `Provider` so an unrecognized
/// value can be turned into a 400 with a readable message instead of a
/// generic JSON-deserialization rejection.
#[derive(Clone, Debug, Default, Deserialize)]
pub struct PutSettings {
    pub provider: Option<String>,
    pub base_url: Option<String>,
    pub model: Option<String>,
    pub max_tokens: Option<u32>,
    pub api_key: Option<String>,
    pub transcribe_base_url: Option<String>,
    pub transcribe_model: Option<String>,
}

/// Everything the server knows about the model to use beyond the env vars
/// and hardcoded defaults. Lives on `AppState` behind a mutex, the same
/// `Arc<Mutex<..>>` pattern every other piece of shared state there follows.
#[derive(Clone, Debug, Default)]
pub struct SettingsState {
    pub provider: Option<Provider>,
    pub base_url: Option<String>,
    pub model: Option<String>,
    pub max_tokens: Option<u32>,
    /// In-memory only. Never appears in `PersistedSettings` or `Resolved`.
    pub api_key: Option<String>,
    /// True once a UI save has set `api_key`, so its reported `source` is
    /// `Ui` rather than `Env` even though the field itself does not change
    /// shape between the two cases.
    pub api_key_from_ui: bool,
    /// Base URL of an OpenAI-compatible `/audio/transcriptions` endpoint.
    /// `None` means voice input is off, which is the default: a microphone
    /// button that records and then has nowhere to send the audio is worse
    /// than no microphone button.
    pub transcribe_base_url: Option<String>,
    pub transcribe_model: Option<String>,
}

/// The non-secret fields plus their provenance, computed once and shared by
/// both public views (`Resolved`, for HTTP; `EffectiveModel`, for the turn).
struct CoreFields {
    provider: Provider,
    provider_source: Source,
    base_url: String,
    base_url_source: Source,
    model: String,
    model_source: Source,
    max_tokens: Option<u32>,
    max_tokens_source: Source,
}

impl SettingsState {
    /// What a freshly started server has: nothing chosen through the UI yet,
    /// and the one secret env var captured once. Every other field is read
    /// live from the environment at resolution time instead (see
    /// `core_fields`), since there is no secrecy reason to freeze them, and
    /// freezing them would stop `ui_setting_overrides_matching_env_var`-style
    /// behavior from working if the env var were exported after the process
    /// started.
    pub fn seeded_from_env() -> Self {
        SettingsState {
            api_key: non_empty_env("ZORP_API_KEY"),
            ..SettingsState::default()
        }
    }

    /// Layer a loaded config file's values in under whatever is already set.
    /// Only ever called once at startup, before a GET or PUT could have
    /// changed anything, so "already set" in practice means "came from the
    /// env-seeded state," which persisted fields never touch anyway.
    pub fn load_persisted(&mut self, persisted: PersistedSettings) {
        if self.provider.is_none() {
            self.provider = persisted.provider.and_then(|s| s.parse().ok());
        }
        if self.base_url.is_none() {
            self.base_url = persisted.base_url;
        }
        if self.model.is_none() {
            self.model = persisted.model;
        }
        if self.max_tokens.is_none() {
            self.max_tokens = persisted.max_tokens;
        }
        if self.transcribe_base_url.is_none() {
            self.transcribe_base_url = persisted.transcribe_base_url;
        }
        if self.transcribe_model.is_none() {
            self.transcribe_model = persisted.transcribe_model;
        }
    }

    /// The shape written to disk. Deliberately cannot carry `api_key`: that
    /// field does not exist on `PersistedSettings`.
    pub fn to_persisted(&self) -> PersistedSettings {
        PersistedSettings {
            provider: self.provider.map(provider_str).map(str::to_string),
            base_url: self.base_url.clone(),
            model: self.model.clone(),
            max_tokens: self.max_tokens,
            transcribe_base_url: self.transcribe_base_url.clone(),
            transcribe_model: self.transcribe_model.clone(),
        }
    }

    /// Apply a validated PUT. Rejects an unknown provider string with a
    /// readable message instead of panicking or silently ignoring it; every
    /// other field is accepted as given. An empty `api_key` clears the
    /// stored key rather than setting it to an empty string.
    pub fn apply(&mut self, put: &PutSettings) -> Result<(), String> {
        if let Some(provider) = &put.provider {
            let parsed: Provider = provider
                .parse()
                .map_err(|e: zorp_agent::BoxErr| e.to_string())?;
            self.provider = Some(parsed);
        }
        if let Some(base_url) = &put.base_url {
            // Same check `fetch_models` applies to the probe. It belongs here
            // more than there: this is the value that gets written to the
            // config file and used to build every later model call, while the
            // probe only reads. Nothing is stored when this fails, so a
            // rejected save leaves the previous setting intact.
            self.base_url = Some(validate_scheme(base_url)?);
        }
        if let Some(model) = &put.model {
            self.model = Some(model.trim().to_string());
        }
        if let Some(max_tokens) = put.max_tokens {
            self.max_tokens = Some(max_tokens);
        }
        if let Some(api_key) = &put.api_key {
            self.api_key = (!api_key.is_empty()).then(|| api_key.clone());
            self.api_key_from_ui = true;
        }
        if let Some(url) = &put.transcribe_base_url {
            // An empty value turns voice input off again, the same way an
            // empty api_key clears the key. Anything else has to be an
            // http(s) URL, checked before it is stored rather than after,
            // so a rejected save leaves the working endpoint in place.
            self.transcribe_base_url = if url.trim().is_empty() {
                None
            } else {
                Some(validate_scheme(url)?)
            };
        }
        if let Some(model) = &put.transcribe_model {
            let model = model.trim();
            self.transcribe_model = (!model.is_empty()).then(|| model.to_string());
        }
        Ok(())
    }

    fn core_fields(&self) -> CoreFields {
        let (provider, provider_source) = match self.provider {
            Some(p) => (p, Source::Ui),
            None => match non_empty_env("ZORP_PROVIDER").and_then(|s| s.parse::<Provider>().ok()) {
                Some(p) => (p, Source::Env),
                None => (Provider::default(), Source::Default),
            },
        };
        let (base_url, base_url_source) = match &self.base_url {
            Some(v) => (v.clone(), Source::Ui),
            None => match non_empty_env("ZORP_BASE_URL") {
                Some(v) => (v, Source::Env),
                None => (DEFAULT_BASE_URL.to_string(), Source::Default),
            },
        };
        let (model, model_source) = match &self.model {
            Some(v) => (v.clone(), Source::Ui),
            None => match non_empty_env("ZORP_MODEL") {
                Some(v) => (v, Source::Env),
                None => (DEFAULT_MODEL.to_string(), Source::Default),
            },
        };
        let (max_tokens, max_tokens_source) = match self.max_tokens {
            Some(v) => (Some(v), Source::Ui),
            None => match non_empty_env("ZORP_MAX_TOKENS").and_then(|s| s.parse::<u32>().ok()) {
                Some(v) => (Some(v), Source::Env),
                None => (None, Source::Default),
            },
        };
        CoreFields {
            provider,
            provider_source,
            base_url,
            base_url_source,
            model,
            model_source,
            max_tokens,
            max_tokens_source,
        }
    }

    /// Where speech goes to be transcribed, and where that choice came
    /// from. Unlike the chat endpoint there is no hardcoded fallback: an
    /// unset value stays unset, because guessing a URL here would mean
    /// offering a microphone that records into nothing.
    fn transcribe_fields(&self) -> (Option<String>, Source, String, Source) {
        let (base_url, base_url_source) = match &self.transcribe_base_url {
            Some(v) => (Some(v.clone()), Source::Ui),
            None => match non_empty_env("ZORP_TRANSCRIBE_BASE_URL") {
                Some(v) => (Some(v), Source::Env),
                None => (None, Source::Default),
            },
        };
        let (model, model_source) = match &self.transcribe_model {
            Some(v) => (v.clone(), Source::Ui),
            None => match non_empty_env("ZORP_TRANSCRIBE_MODEL") {
                Some(v) => (v, Source::Env),
                None => (DEFAULT_TRANSCRIBE_MODEL.to_string(), Source::Default),
            },
        };
        (base_url, base_url_source, model, model_source)
    }

    /// The endpoint `POST /api/transcribe` forwards to, or `None` when
    /// voice input is switched off.
    pub fn transcription(&self) -> Option<Transcription> {
        let (base_url, _, model, _) = self.transcribe_fields();
        base_url.map(|base_url| Transcription { base_url, model })
    }

    fn api_key_provenance(&self) -> (bool, Source) {
        match &self.api_key {
            Some(_) if self.api_key_from_ui => (true, Source::Ui),
            Some(_) => (true, Source::Env),
            None => (false, Source::Default),
        }
    }

    /// "Nothing at all is set" is exactly the shape that used to fail
    /// silently on the first message: no chosen provider/base_url/model and
    /// no key, meaning every field is the hardcoded default. Anything else
    /// means someone did something intentional, and the turn is worth
    /// trying rather than refusing up front.
    fn configured(fields: &CoreFields, has_api_key: bool) -> bool {
        has_api_key
            || fields.provider_source != Source::Default
            || fields.base_url_source != Source::Default
            || fields.model_source != Source::Default
    }

    /// The HTTP-safe view: what `GET`/`PUT /api/settings` answer with. Has
    /// no field that could carry the API key itself.
    pub fn resolve(&self) -> Resolved {
        let fields = self.core_fields();
        let (has_api_key, api_key_source) = self.api_key_provenance();
        let configured = Self::configured(&fields, has_api_key);
        let (transcribe_base_url, transcribe_base_url_source, transcribe_model, transcribe_model_source) =
            self.transcribe_fields();
        // Whether audio stays on this machine, decided here rather than in
        // the browser, because the browser cannot see this URL any other
        // way and the answer is the one thing a user needs before speaking.
        let transcribe_local = transcribe_base_url
            .as_deref()
            .map(is_loopback_url)
            .unwrap_or(true);
        Resolved {
            provider: fields.provider,
            provider_source: fields.provider_source,
            base_url: fields.base_url,
            base_url_source: fields.base_url_source,
            model: fields.model,
            model_source: fields.model_source,
            max_tokens: fields.max_tokens,
            max_tokens_source: fields.max_tokens_source,
            has_api_key,
            api_key_source,
            configured,
            transcribe_configured: transcribe_base_url.is_some(),
            transcribe_base_url: transcribe_base_url.unwrap_or_default(),
            transcribe_base_url_source,
            transcribe_model,
            transcribe_model_source,
            transcribe_local,
        }
    }

    /// What the turn actually builds `HttpModel` from, api key included.
    /// Never serialized: nothing outside the server process should see this.
    pub fn effective_model(&self) -> EffectiveModel {
        let fields = self.core_fields();
        let (has_api_key, _) = self.api_key_provenance();
        let configured = Self::configured(&fields, has_api_key);
        EffectiveModel {
            provider: fields.provider,
            base_url: fields.base_url,
            model: fields.model,
            max_tokens: fields.max_tokens,
            api_key: self.api_key.clone(),
            configured,
        }
    }
}

fn non_empty_env(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|s| !s.is_empty())
}

/// What `GET`/`PUT /api/settings` answer with: the effective configuration
/// plus enough provenance for the UI to explain it (e.g. "from ZORP_MODEL").
/// `api_key` itself is never a field here; only `has_api_key` is.
#[derive(Clone, Debug, Serialize)]
pub struct Resolved {
    pub provider: Provider,
    pub provider_source: Source,
    pub base_url: String,
    pub base_url_source: Source,
    pub model: String,
    pub model_source: Source,
    pub max_tokens: Option<u32>,
    pub max_tokens_source: Source,
    pub has_api_key: bool,
    pub api_key_source: Source,
    pub configured: bool,
    /// Whether there is anywhere to send recorded speech. False means the
    /// UI offers an explanation instead of a microphone.
    pub transcribe_configured: bool,
    /// Empty when unset. Shown in the settings panel, and shown next to the
    /// microphone when it is not loopback, because that is the case where
    /// speaking sends audio off this machine.
    pub transcribe_base_url: String,
    pub transcribe_base_url_source: Source,
    pub transcribe_model: String,
    pub transcribe_model_source: Source,
    pub transcribe_local: bool,
}

/// Where `POST /api/transcribe` forwards audio. Not serialized: the browser
/// reads the same values off `Resolved`.
pub struct Transcription {
    pub base_url: String,
    pub model: String,
}

/// Whether a URL points back at this machine.
///
/// The whole privacy claim for voice input rests on this being right, so it
/// handles the shapes a person actually types: a port, IPv6 in brackets,
/// `127.0.0.1` and the rest of `127/8`, and userinfo before the host.
/// Anything it cannot read confidently is treated as remote, which errs
/// towards warning too often rather than too rarely.
pub fn is_loopback_url(url: &str) -> bool {
    let after_scheme = url.split_once("://").map(|(_, rest)| rest).unwrap_or(url);
    let authority = after_scheme.split(['/', '?', '#']).next().unwrap_or("");
    let authority = authority
        .rsplit_once('@')
        .map(|(_, host)| host)
        .unwrap_or(authority);
    let host = match authority.strip_prefix('[') {
        Some(rest) => rest.split(']').next().unwrap_or(""),
        None => authority.split(':').next().unwrap_or(""),
    }
    .trim()
    .to_ascii_lowercase();

    if host == "localhost" || host.ends_with(".localhost") {
        return true;
    }
    if let Ok(v4) = host.parse::<std::net::Ipv4Addr>() {
        return v4.is_loopback();
    }
    if let Ok(v6) = host.parse::<std::net::Ipv6Addr>() {
        return v6.is_loopback();
    }
    false
}

/// What `turn::run_agent` builds `HttpModel` from. Carries the real API key
/// and is never serialized as a whole.
pub struct EffectiveModel {
    pub provider: Provider,
    pub base_url: String,
    pub model: String,
    pub max_tokens: Option<u32>,
    pub api_key: Option<String>,
    pub configured: bool,
}

/// Where the persisted settings file lives. `ZORP_WEB_CONFIG` overrides it
/// entirely; otherwise `$XDG_CONFIG_HOME/zorp/web.toml`, falling back to
/// `$HOME/.config/zorp/web.toml`, and finally `.zorp-config/zorp/web.toml`
/// if neither is set. Mirrors the `state_path` helper `zorp-agent` already
/// uses for its own state files (`ZORP_STATE_DB`, `ZORP_TRUST_FILE`).
pub fn config_path() -> PathBuf {
    if let Some(p) = non_empty_env(CONFIG_PATH_VAR) {
        return PathBuf::from(p);
    }
    let base = non_empty_env("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| non_empty_env("HOME").map(|h| PathBuf::from(h).join(".config")))
        .unwrap_or_else(|| PathBuf::from(".zorp-config"));
    base.join("zorp").join("web.toml")
}

/// Read the persisted, non-secret settings, if the file exists and parses.
/// A missing file is not an error: the feature is opt-in by nature of never
/// having been saved yet. A corrupt file is logged and ignored rather than
/// blocking startup over a config file, of all things.
pub fn load() -> Option<PersistedSettings> {
    let path = config_path();
    let text = std::fs::read_to_string(&path).ok()?;
    match toml::from_str(&text) {
        Ok(settings) => Some(settings),
        Err(e) => {
            eprintln!("zorp-web: ignoring unreadable {}: {e}", path.display());
            None
        }
    }
}

/// Write the non-secret settings, creating the parent directory if needed.
pub fn save(settings: &PersistedSettings) -> io::Result<()> {
    let path = config_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let text = toml::to_string_pretty(settings)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    std::fs::write(&path, text)
}

/// What listing or testing a model endpoint came back with. `error` is a
/// short, human-readable reason and is `None` on success; `models` is empty
/// on any failure. There is deliberately no "this failed" variant that isn't
/// just this: the whole point is that a caller-supplied endpoint being
/// unreachable is not a server error.
pub struct ModelsResult {
    pub models: Vec<String>,
    pub error: Option<String>,
}

/// How long to wait for a models-listing/test-connection probe. Short on
/// purpose: this is a UI convenience the user is actively waiting on, not a
/// long-running model call, and a local model server that will not accept a
/// connection in a couple of seconds is not going to start accepting one
/// later in the same request.
const PROBE_CONNECT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(3);
const PROBE_READ_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

/// Proxy `GET {base_url}/models` and return the model ids from an
/// OpenAI-shaped `{"data":[{"id":...}, ...]}` response.
///
/// The server fetching a caller-supplied URL is server-side request forgery
/// in shape: nothing stops a caller from pointing `base_url` at another
/// service on the same machine or network and reading back whatever answers
/// on `/models`. The risk is bounded, not absent: `zorp-web` binds loopback
/// by default and requires a token otherwise (`zorp-web/src/auth.rs`), so
/// reaching this endpoint at all already implies either local access or a
/// valid token, and the scheme restriction below at least rules out
/// `file://` and friends. An operator who exposes `zorp-web` on a network
/// without a token has a bigger problem than this one endpoint.
pub fn fetch_models(base_url: &str) -> ModelsResult {
    let base_url = match validate_scheme(base_url) {
        Ok(u) => u,
        Err(e) => {
            return ModelsResult {
                models: Vec::new(),
                error: Some(e),
            }
        }
    };
    let url = zorp_agent::join_url(&base_url, "models");
    let agent = ureq::AgentBuilder::new()
        .timeout_connect(PROBE_CONNECT_TIMEOUT)
        .timeout_read(PROBE_READ_TIMEOUT)
        .build();
    match agent.get(&url).call() {
        Ok(resp) => match resp.into_json::<serde_json::Value>() {
            Ok(body) => {
                let models = body
                    .get("data")
                    .and_then(|d| d.as_array())
                    .map(|items| {
                        items
                            .iter()
                            .filter_map(|m| {
                                m.get("id").and_then(|v| v.as_str()).map(str::to_string)
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                ModelsResult {
                    models,
                    error: None,
                }
            }
            Err(e) => ModelsResult {
                models: Vec::new(),
                error: Some(format!("{url} did not answer with JSON: {e}")),
            },
        },
        Err(ureq::Error::Status(code, resp)) => ModelsResult {
            models: Vec::new(),
            error: Some(format!(
                "{url}: status code {code}: {}",
                resp.into_string().unwrap_or_default()
            )),
        },
        // No `{url}` prefix here. ureq's transport errors already start with
        // the URL, and prefixing produced "http://…/models: http://…/models:
        // Connection Failed: …", which reads like two different failures.
        Err(e) => ModelsResult {
            models: Vec::new(),
            error: Some(e.to_string()),
        },
    }
}

fn validate_scheme(base_url: &str) -> Result<String, String> {
    let trimmed = base_url.trim();
    if trimmed.is_empty() {
        return Err("no base URL is configured".to_string());
    }
    if !(trimmed.starts_with("http://") || trimmed.starts_with("https://")) {
        return Err("base URL must start with http:// or https://".to_string());
    }
    Ok(trimmed.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clean_state_resolves_to_hardcoded_defaults_and_not_configured() {
        // Isolate from whatever the running process happens to have
        // exported; other tests in this workspace also touch these vars.
        for var in [
            "ZORP_PROVIDER",
            "ZORP_BASE_URL",
            "ZORP_MODEL",
            "ZORP_API_KEY",
            "ZORP_MAX_TOKENS",
        ] {
            std::env::remove_var(var);
        }
        let resolved = SettingsState::default().resolve();
        assert_eq!(resolved.provider, Provider::OpenAiCompatible);
        assert_eq!(resolved.base_url, DEFAULT_BASE_URL);
        assert_eq!(resolved.model, DEFAULT_MODEL);
        assert!(!resolved.has_api_key);
        assert!(!resolved.configured);
        assert_eq!(resolved.provider_source, Source::Default);
        assert_eq!(resolved.base_url_source, Source::Default);
        assert_eq!(resolved.model_source, Source::Default);
        assert_eq!(resolved.api_key_source, Source::Default);
    }

    #[test]
    fn apply_rejects_unknown_provider_without_mutating_state() {
        let mut state = SettingsState::default();
        let put = PutSettings {
            provider: Some("bedrock".to_string()),
            ..PutSettings::default()
        };
        let err = state.apply(&put).unwrap_err();
        assert!(err.contains("bedrock"), "message: {err}");
        assert!(state.provider.is_none());
    }

    #[test]
    fn to_persisted_never_carries_the_api_key() {
        let mut state = SettingsState::default();
        state
            .apply(&PutSettings {
                base_url: Some("http://localhost:11434/v1".to_string()),
                model: Some("qwen3:4b".to_string()),
                api_key: Some("sk-secret".to_string()),
                ..PutSettings::default()
            })
            .unwrap();
        let toml_text = toml::to_string(&state.to_persisted()).unwrap();
        assert!(!toml_text.contains("sk-secret"));
        assert!(!toml_text.to_lowercase().contains("api_key"));
    }

    #[test]
    fn an_empty_api_key_put_clears_the_stored_key() {
        let mut state = SettingsState::seeded_from_env();
        state.api_key = Some("leftover".to_string());
        state
            .apply(&PutSettings {
                api_key: Some(String::new()),
                ..PutSettings::default()
            })
            .unwrap();
        assert!(state.api_key.is_none());
        assert!(state.api_key_from_ui);
    }

    #[test]
    fn validate_scheme_rejects_non_http_urls() {
        assert!(validate_scheme("file:///etc/passwd").is_err());
        assert!(validate_scheme("ftp://example.com").is_err());
        assert!(validate_scheme("").is_err());
        assert!(validate_scheme("http://localhost:11434/v1").is_ok());
        assert!(validate_scheme("https://api.openai.com/v1").is_ok());
    }

    /// The scheme check guarded only `fetch_models`, which is the read-only
    /// probe. A save went through unchecked, so `file:///etc/passwd` could be
    /// written to `~/.config/zorp/web.toml` and handed to the model call on
    /// every later turn. The stricter check was on the harmless path and the
    /// looser one on the path that persists. Rejecting here turns it into the
    /// same 400 an unknown provider gets.
    #[test]
    fn a_save_rejects_a_base_url_that_is_not_http() {
        for bad in ["file:///etc/passwd", "ftp://example.com", "  "] {
            let mut state = SettingsState::default();
            let err = state
                .apply(&PutSettings {
                    base_url: Some(bad.to_string()),
                    ..PutSettings::default()
                })
                .expect_err("{bad} was accepted");
            assert!(
                err.contains("http://") || err.contains("no base URL"),
                "unhelpful message for {bad}: {err}"
            );
            assert!(
                state.base_url.is_none(),
                "{bad} was stored anyway despite the error"
            );
        }
    }

    /// The mic button's warning label is driven by this, so a host it reads
    /// as local when it is not means someone is told their voice stays on
    /// their machine while it does not.
    #[test]
    fn loopback_urls_are_told_apart_from_everything_else() {
        for local in [
            "http://127.0.0.1:8080/v1",
            "http://localhost:8080/v1",
            "http://LOCALHOST:8080",
            "http://127.5.4.3/v1",
            "http://[::1]:8080/v1",
            "http://user:pw@127.0.0.1:8080/v1",
        ] {
            assert!(is_loopback_url(local), "{local} was read as remote");
        }
        for remote in [
            "https://api.openai.com/v1",
            "http://192.168.1.20:8080/v1",
            "http://speech.example.com/v1",
            // The host is example.com, not localhost. Reading left to right
            // and stopping at the first familiar word is how this goes
            // wrong.
            "http://localhost.example.com/v1",
            "http://127.0.0.1.example.com/v1",
        ] {
            assert!(!is_loopback_url(remote), "{remote} was read as local");
        }
    }

    #[test]
    fn a_transcription_endpoint_survives_a_write_and_a_read() {
        let mut state = SettingsState::default();
        state
            .apply(&PutSettings {
                transcribe_base_url: Some("http://127.0.0.1:8080/v1".to_string()),
                transcribe_model: Some("whisper-1".to_string()),
                ..PutSettings::default()
            })
            .unwrap();
        let text = toml::to_string(&state.to_persisted()).unwrap();
        let read_back: PersistedSettings = toml::from_str(&text).unwrap();

        let mut restored = SettingsState::default();
        restored.load_persisted(read_back);
        let endpoint = restored.transcription().expect("endpoint was not restored");
        assert_eq!(endpoint.base_url, "http://127.0.0.1:8080/v1");
        assert_eq!(endpoint.model, "whisper-1");
    }

    /// Voice input has to be switchable off again, and the panel's way of
    /// saying so is an empty field.
    #[test]
    fn an_empty_transcription_url_switches_voice_input_off() {
        let mut state = SettingsState::default();
        state
            .apply(&PutSettings {
                transcribe_base_url: Some("http://127.0.0.1:8080/v1".to_string()),
                ..PutSettings::default()
            })
            .unwrap();
        state
            .apply(&PutSettings {
                transcribe_base_url: Some(String::new()),
                ..PutSettings::default()
            })
            .unwrap();
        assert!(state.transcription().is_none());
        assert!(!state.resolve().transcribe_configured);
    }

    /// Configuring speech to text must not make the chat model look
    /// configured, or the composer stops warning about the thing that
    /// actually blocks a message.
    #[test]
    fn a_transcription_endpoint_does_not_configure_the_chat_model() {
        for var in ["ZORP_PROVIDER", "ZORP_BASE_URL", "ZORP_MODEL", "ZORP_API_KEY"] {
            std::env::remove_var(var);
        }
        let mut state = SettingsState::default();
        state
            .apply(&PutSettings {
                transcribe_base_url: Some("http://127.0.0.1:8080/v1".to_string()),
                ..PutSettings::default()
            })
            .unwrap();
        let resolved = state.resolve();
        assert!(resolved.transcribe_configured);
        assert!(!resolved.configured, "voice input configured the chat model");
    }

    /// The good case, so the check above cannot be satisfied by rejecting
    /// everything. Ollama's URL is the one this feature exists to accept.
    #[test]
    fn a_save_still_accepts_an_ordinary_http_base_url() {
        let mut state = SettingsState::default();
        state
            .apply(&PutSettings {
                base_url: Some("  http://localhost:11434/v1  ".to_string()),
                ..PutSettings::default()
            })
            .unwrap();
        assert_eq!(
            state.base_url.as_deref(),
            Some("http://localhost:11434/v1"),
            "the surrounding whitespace should still be trimmed"
        );
    }
}
