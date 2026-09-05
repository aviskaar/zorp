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
    /// The directory the agent works in, as `PUT /api/workspace` last
    /// stored it. Written to disk, unlike `api_key`, because it is a path
    /// and not a secret: somebody reading this file learns where their own
    /// work lives, which they already knew. Not persisting it would mean
    /// choosing a directory again after every restart, and a workspace
    /// nobody chose is exactly what this feature exists to stop.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub workspace: Option<String>,
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
    /// The saved workspace path, the lowest-precedence source of the
    /// directory the agent works in. `--workspace` and `ZORP_WORKSPACE`
    /// beat it; see `crate::workspace`. It lives here because this is the
    /// state that gets written to the settings file, and it is deliberately
    /// not a field on `PutSettings`: a workspace is validated before it is
    /// stored, and `PUT /api/workspace` is the one door that does it.
    pub workspace: Option<String>,
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
        if self.workspace.is_none() {
            self.workspace = persisted.workspace;
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
            workspace: self.workspace.clone(),
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
///
/// `details` carries the same models in the same order, plus whatever else
/// the endpoint said about each one. It is additive: `models` keeps the
/// shape every existing caller reads, and a provider that says nothing but
/// an id (Ollama, oMLX, most OpenAI-compatible servers) produces details
/// with only `id` set.
#[derive(Default)]
pub struct ModelsResult {
    pub models: Vec<String>,
    pub details: Vec<ModelDetail>,
    pub error: Option<String>,
}

/// One model, as the endpoint described it.
///
/// Everything past `id` is optional because only some providers say it, and
/// the difference between "this costs nothing" and "nobody said what this
/// costs" is the whole reason this type exists. `Some(0.0)` is a provider
/// stating a price of zero. `None` is a provider stating nothing, and a UI
/// that turns the second into the first is telling someone a model is free
/// on no evidence.
///
/// Prices are per token, as the provider stated them, and are not compared
/// across providers or converted into anything. OpenRouter uses a negative
/// price for a model whose cost is decided per request (`openrouter/auto`),
/// which is neither free nor a stated price, and it stays negative here.
#[derive(Clone, Debug, Default, Serialize)]
pub struct ModelDetail {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context_length: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompt_price: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub completion_price: Option<f64>,
    /// What the model answers with, when the listing says. OpenRouter
    /// serves image and audio models beside the chat ones and separates
    /// them only here, so without this a picker sorting on context window
    /// can land on a music model. `None` is a provider that said nothing,
    /// which is not the same as one that said "not text".
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_modalities: Option<Vec<String>>,
}

/// Read an OpenAI-shaped `{"data":[{"id":...}, ...]}` listing.
///
/// Split out of `fetch_models` so the parsing has a test that does not need
/// a network. Anything without a string `id` is dropped: an entry nobody can
/// name is an entry nobody can select.
fn parse_models(body: &serde_json::Value) -> (Vec<String>, Vec<ModelDetail>) {
    let Some(items) = body.get("data").and_then(|d| d.as_array()) else {
        return (Vec::new(), Vec::new());
    };
    let mut models = Vec::new();
    let mut details = Vec::new();
    for item in items {
        let Some(id) = item.get("id").and_then(|v| v.as_str()) else {
            continue;
        };
        models.push(id.to_string());
        details.push(ModelDetail {
            id: id.to_string(),
            name: item
                .get("name")
                .and_then(|v| v.as_str())
                .map(str::to_string),
            context_length: item.get("context_length").and_then(|v| v.as_u64()),
            prompt_price: price(item, "prompt"),
            completion_price: price(item, "completion"),
            output_modalities: modalities(item),
        });
    }
    (models, details)
}

/// What the model answers with, out of OpenRouter's `architecture` object.
/// An empty or non-array value reads as nothing said, because a provider
/// that listed no modalities has not ruled text out.
fn modalities(item: &serde_json::Value) -> Option<Vec<String>> {
    let listed: Vec<String> = item
        .get("architecture")?
        .get("output_modalities")?
        .as_array()?
        .iter()
        .filter_map(|v| v.as_str())
        .map(str::to_string)
        .collect();
    (!listed.is_empty()).then_some(listed)
}

/// One price out of OpenRouter's `pricing` object. It sends them as decimal
/// strings ("0", "0.0000004"), so a number is accepted too rather than
/// depending on a wire detail nobody promised.
fn price(item: &serde_json::Value, field: &str) -> Option<f64> {
    let value = item.get("pricing")?.get(field)?;
    match value {
        serde_json::Value::String(s) => s.trim().parse().ok(),
        other => other.as_f64(),
    }
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
///
/// The key rides along as a bearer token when there is one, because a
/// locally protected server (oMLX with `--api-key`, for one) answers its
/// listing with a 401 without it, and the settings panel then looks
/// unable to connect to a server that is running fine. Sending a key to a
/// caller-supplied URL is the same exposure `test_connection` already
/// accepts, and it is bounded the same way.
pub fn fetch_models(base_url: &str, api_key: Option<&str>) -> ModelsResult {
    let base_url = match validate_scheme(base_url) {
        Ok(u) => u,
        Err(e) => {
            return ModelsResult {
                error: Some(e),
                ..ModelsResult::default()
            }
        }
    };
    let url = zorp_agent::join_url(&base_url, "models");
    let agent = ureq::AgentBuilder::new()
        .timeout_connect(PROBE_CONNECT_TIMEOUT)
        .timeout_read(PROBE_READ_TIMEOUT)
        .build();
    let mut req = agent.get(&url);
    if let Some(key) = api_key {
        req = req.set("Authorization", &format!("Bearer {key}"));
    }
    match req.call() {
        Ok(resp) => match resp.into_json::<serde_json::Value>() {
            Ok(body) => {
                let (models, details) = parse_models(&body);
                ModelsResult {
                    models,
                    details,
                    error: None,
                }
            }
            Err(e) => ModelsResult {
                error: Some(format!("{url} did not answer with JSON: {e}")),
                ..ModelsResult::default()
            },
        },
        Err(ureq::Error::Status(code, resp)) => ModelsResult {
            error: Some(format!(
                "{url}: status code {code}: {}",
                resp.into_string().unwrap_or_default()
            )),
            ..ModelsResult::default()
        },
        // No `{url}` prefix here. ureq's transport errors already start with
        // the URL, and prefixing produced "http://…/models: http://…/models:
        // Connection Failed: …", which reads like two different failures.
        Err(e) => ModelsResult {
            error: Some(e.to_string()),
            ..ModelsResult::default()
        },
    }
}

/// A reasoning model can think for a while before emitting its first
/// token, and this probe waits for a whole (tiny) completion rather than
/// a listing. Longer than `PROBE_READ_TIMEOUT` on purpose.
const PROBE_COMPLETION_READ_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

/// Ask the configured endpoint for the smallest possible completion, with
/// the configured credentials, and report whether it worked.
///
/// This is what "test connection" has to mean. The previous probe listed
/// models over an unauthenticated `GET /models`, which cannot fail for a
/// bad key on any provider whose listing is public, and OpenRouter's is:
/// a deliberately invalid key returned `{"ok": true}`. Listing also says
/// nothing about whether the *configured model* can actually be called.
///
/// One request covers all four things that are usually wrong: the address,
/// the credentials, the model name, and the provider's wire format.
///
/// The body is deliberately minimal. `max_tokens: 1` keeps a paid endpoint
/// to a fraction of a cent, and the prompt is one character because
/// nothing reads the answer; only the status code is inspected.
pub fn probe_completion(
    base_url: &str,
    provider: Provider,
    model: &str,
    api_key: Option<&str>,
) -> Result<(), String> {
    let base_url = validate_scheme(base_url)?;
    let url = zorp_agent::join_url(&base_url, provider.path_suffix());

    let body = match provider {
        Provider::OpenAiCompatible => serde_json::json!({
            "model": model,
            "messages": [{"role": "user", "content": "hi"}],
            "max_tokens": 1,
        }),
        // Anthropic requires max_tokens and takes no system role here.
        Provider::Anthropic => serde_json::json!({
            "model": model,
            "messages": [{"role": "user", "content": "hi"}],
            "max_tokens": 1,
        }),
    };

    let agent = ureq::AgentBuilder::new()
        .timeout_connect(PROBE_CONNECT_TIMEOUT)
        .timeout_read(PROBE_COMPLETION_READ_TIMEOUT)
        .build();

    let mut req = agent.post(&url).set("content-type", "application/json");
    // Sent per provider, matching what `HttpModel` puts on a real turn. A
    // probe that authenticates differently from the thing it is probing
    // would be testing the wrong request.
    match provider {
        Provider::OpenAiCompatible => {
            if let Some(key) = api_key {
                req = req.set("Authorization", &format!("Bearer {key}"));
            }
        }
        Provider::Anthropic => {
            if let Some(key) = api_key {
                req = req.set("x-api-key", key);
            }
            req = req.set("anthropic-version", "2023-06-01");
        }
    }

    match req.send_json(body) {
        Ok(_) => Ok(()),
        Err(ureq::Error::Status(code, resp)) => {
            let detail = resp.into_string().unwrap_or_default();
            // Truncated: a provider can answer an error with an HTML page,
            // and the whole of it in a toast helps nobody.
            let detail: String = detail.chars().take(300).collect();
            Err(format!("{url}: status code {code}: {detail}"))
        }
        // ureq's transport errors already begin with the URL.
        Err(e) => Err(e.to_string()),
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

    /// A provider that says only an id keeps working exactly as it did.
    /// This is Ollama, oMLX, and most OpenAI-compatible servers: `models`
    /// is the same list it always was, and every detail carries no price,
    /// which is what tells the UI not to sort them into free and paid.
    #[test]
    fn a_listing_with_only_ids_reports_no_prices() {
        let body = serde_json::json!({"data": [{"id": "qwen3:4b"}, {"id": "llama3.2"}]});
        let (models, details) = parse_models(&body);
        assert_eq!(models, vec!["qwen3:4b", "llama3.2"]);
        assert_eq!(details.len(), 2);
        assert!(details.iter().all(|d| d.prompt_price.is_none()));
        assert!(details.iter().all(|d| d.completion_price.is_none()));
        assert!(details.iter().all(|d| d.context_length.is_none()));
    }

    /// OpenRouter's shape. The prices arrive as decimal strings, a free
    /// model states zero for both, and `openrouter/auto` states a negative
    /// price because its cost is decided per request. All three have to
    /// come out different: "free", "costs this much" and "nobody said" are
    /// three different facts and the UI groups on them.
    #[test]
    fn openrouter_prices_are_parsed_out_of_the_pricing_object() {
        let body = serde_json::json!({"data": [
            {
                "id": "meta-llama/llama-3.3-70b-instruct:free",
                "name": "Llama 3.3 70B Instruct (free)",
                "context_length": 65536,
                "pricing": {"prompt": "0", "completion": "0"},
            },
            {
                "id": "anthropic/claude-sonnet-4",
                "name": "Claude Sonnet 4",
                "context_length": 200000,
                "pricing": {"prompt": "0.000003", "completion": "0.000015"},
            },
            {
                "id": "openrouter/auto",
                "name": "Auto Router",
                "pricing": {"prompt": "-1", "completion": "-1"},
            },
            {
                "id": "google/lyria-3-clip-preview",
                "name": "Lyria 3 Clip Preview",
                "context_length": 1048576,
                "pricing": {"prompt": "0", "completion": "0"},
                "architecture": {"output_modalities": ["text", "audio"]},
            },
        ]});
        let (models, details) = parse_models(&body);
        assert_eq!(models.len(), 4);
        // A free model that answers with audio is still free and still
        // listed. What it is not is a chat model, and only the listing
        // says so.
        assert_eq!(
            details[3].output_modalities.as_deref(),
            Some(["text".to_string(), "audio".to_string()].as_slice())
        );
        assert!(details[0].output_modalities.is_none());
        assert_eq!(details[0].prompt_price, Some(0.0));
        assert_eq!(details[0].completion_price, Some(0.0));
        assert_eq!(details[0].context_length, Some(65536));
        assert_eq!(
            details[0].name.as_deref(),
            Some("Llama 3.3 70B Instruct (free)")
        );
        assert_eq!(details[1].prompt_price, Some(0.000003));
        assert_eq!(details[2].prompt_price, Some(-1.0));
    }

    /// A price sent as a number rather than a string still reads. Nobody
    /// promised the string form and a listing that used numbers would
    /// otherwise look like a listing that stated no price at all.
    #[test]
    fn a_numeric_price_reads_the_same_as_a_string_one() {
        let body =
            serde_json::json!({"data": [{"id": "x", "pricing": {"prompt": 0, "completion": 0}}]});
        let (_, details) = parse_models(&body);
        assert_eq!(details[0].prompt_price, Some(0.0));
    }

    /// An entry with no id is dropped rather than listed as an empty
    /// string, and it must not shift `models` and `details` out of step.
    #[test]
    fn an_entry_with_no_id_is_dropped_from_both_lists() {
        let body = serde_json::json!({"data": [{"id": "a"}, {"name": "no id here"}, {"id": "b"}]});
        let (models, details) = parse_models(&body);
        assert_eq!(models, vec!["a", "b"]);
        assert_eq!(details.len(), 2);
        assert_eq!(details[1].id, "b");
    }

    /// A body that is JSON but not a listing is an empty list, not a panic.
    #[test]
    fn a_body_with_no_data_array_lists_nothing() {
        let (models, details) = parse_models(&serde_json::json!({"error": "nope"}));
        assert!(models.is_empty());
        assert!(details.is_empty());
    }

    /// The workspace is a path and not a secret, so unlike the API key it
    /// survives a restart. A person who chose a directory in the browser
    /// should not have to choose it again tomorrow.
    #[test]
    fn the_workspace_round_trips_through_the_settings_file() {
        let dir = tempfile::tempdir().unwrap();
        let config = dir.path().join("web.toml");
        std::env::set_var(CONFIG_PATH_VAR, &config);

        let state = SettingsState {
            workspace: Some("/home/someone/research".to_string()),
            ..SettingsState::default()
        };
        save(&state.to_persisted()).unwrap();

        let mut fresh = SettingsState::default();
        fresh.load_persisted(load().unwrap());
        assert_eq!(fresh.workspace.as_deref(), Some("/home/someone/research"));

        std::env::remove_var(CONFIG_PATH_VAR);
    }

    /// Precedence, in one test because the environment variable it sets is
    /// process wide. The flag beats the variable, the variable beats what
    /// was saved, and nothing at all beats none of them: there is no
    /// fallback to the current directory, which is the whole point.
    #[test]
    fn the_flag_beats_the_variable_and_the_variable_beats_the_saved_path() {
        use crate::workspace::{resolve, Source, Unusable};
        let dir = tempfile::tempdir().unwrap();
        let flag = dir.path().join("from-flag");
        let env = dir.path().join("from-env");
        let saved = dir.path().join("from-saved");
        for path in [&flag, &env, &saved] {
            std::fs::create_dir(path).unwrap();
        }
        let saved = saved.to_string_lossy().into_owned();

        std::env::remove_var(crate::workspace::ENV_VAR);
        let chosen = resolve(None, Some(&saved)).unwrap();
        assert_eq!(chosen.source, Source::Saved);

        std::env::set_var(crate::workspace::ENV_VAR, &env);
        let chosen = resolve(None, Some(&saved)).unwrap();
        assert_eq!(chosen.source, Source::Env);
        assert_eq!(chosen.path, env.canonicalize().unwrap());

        let chosen = resolve(Some(&flag), Some(&saved)).unwrap();
        assert_eq!(chosen.source, Source::Flag);
        assert_eq!(chosen.path, flag.canonicalize().unwrap());

        std::env::remove_var(crate::workspace::ENV_VAR);
        assert!(matches!(resolve(None, None), Err(Unusable::Unset)));
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
