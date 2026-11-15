//! The saved configuration both surfaces read.
//!
//! Setting zorp up in the browser and setting it up in the terminal used to
//! be two separate jobs. The browser's first run flow asks which provider
//! you want, lists what the endpoint offers, tests it, and saves the answer
//! to a file. The terminal read none of that: it had its own chain of
//! flags, environment variables and flavor manifests, and its own default.
//!
//! The two already share the session store. This is the last thing that was
//! duplicated, and it is the first thing a new person hits.
//!
//! **The API key is not here and must never be.** There is no field for it
//! on `Saved`, so there is nothing on the struct to serialize a secret
//! through by accident. The key lives in memory and in `ZORP_API_KEY`, which
//! both surfaces already read, so the secret was the one thing that was
//! already shared.
//!
//! `workspace` is written here but is the browser's alone. The CLI works in
//! the directory it was started in, which is a directory the person chose by
//! standing in it. See `docs/DECISIONS.md` (2026-09-05). A CLI that read
//! this field and moved itself would be a surprise rather than a
//! convenience, so nothing in `zorp-agent` reads it.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Overrides the location of the saved settings.
pub const PATH_VAR: &str = "ZORP_CONFIG";

/// The variable the browser used before this file was shared. Still read,
/// so an existing setup keeps working.
pub const LEGACY_PATH_VAR: &str = "ZORP_WEB_CONFIG";

/// The file this writes.
const FILE: &str = "zorp.toml";

/// What the browser wrote before the file was shared between the two
/// surfaces. Read when `zorp.toml` is not there, never written.
const LEGACY_FILE: &str = "web.toml";

/// The only shape ever written to the settings file.
///
/// No `api_key` field exists here on purpose: there is nothing on this
/// struct to accidentally serialize a secret through.
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
pub struct Saved {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,
    /// The directory the browser's agent works in. Written by `zorp-web`
    /// and read by `zorp-web`. The CLI never reads it; see the module doc.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub workspace: Option<String>,
    /// The text-to-speech model and voice, the way the chat model is a
    /// setting. Both surfaces share the file; today only the browser speaks,
    /// but the fields live here so the terminal could later. Phonon is the
    /// only model as of this writing, but it is selectable on purpose.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub speech_model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub speech_voice: Option<String>,
    /// The OS-level sandbox the agent's shell commands run under, as a mode
    /// name (`off`, `workspace`, `build`, `read-only`). The person's own
    /// choice, so it is trusted; a flavor's request to loosen it is not (see
    /// ADR 0141). A string rather than the enum so an unknown value in a file
    /// a person hand-edited is reported where it is read rather than failing
    /// to deserialize the whole settings file.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sandbox: Option<String>,
    /// The model speed dial: up to nine numbered slots, each a model a
    /// person parked for a one-chord switch. Both surfaces read it from the
    /// same file. See issue #242 and `docs/DECISIONS.md` (2026-11-02).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub dial: Vec<DialSlot>,
    /// Named model connections a conversation can run on instead of the
    /// model above: a local Ollama, a remote one, OpenRouter, an oMLX box.
    /// Saved together so every one is available at once, and picked per
    /// conversation. See `docs/DECISIONS.md` (2026-11-15).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub connections: Vec<Connection>,
    /// The connection a new conversation starts on, by id. Unset means the
    /// model above, which is what every conversation used before.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_connection: Option<String>,
}

/// One saved way of reaching a model: an endpoint, a provider shape and the
/// model to use there unless a conversation picks another.
///
/// **It holds no key, and there is no field for one**, the same as
/// `DialSlot`. Each connection's key lives in the server's memory and, in
/// the Mac app, the login keychain. `api_key_env` is the *name* of an
/// environment variable to read a key from, for the terminal and for a
/// browser server started with keys in its environment; a name is not a
/// secret. A connection never falls back to `ZORP_API_KEY`: that key was
/// given for the default endpoint, and sending it to whatever host a
/// connection names would hand one provider's secret to another.
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
pub struct Connection {
    /// The connection's identity in this file and on a session row:
    /// lowercase letters, digits, `-` and `_`, at most 40 characters.
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    pub base_url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_key_env: Option<String>,
}

impl Connection {
    /// What a person reads: the label, or the id when there is none.
    pub fn name(&self) -> &str {
        self.label
            .as_deref()
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .unwrap_or(&self.id)
    }

    /// The key named by `api_key_env`, when that variable is set.
    pub fn env_key(&self) -> Option<String> {
        self.api_key_env
            .as_deref()
            .and_then(|name| std::env::var(name).ok())
            .filter(|k| !k.is_empty())
    }
}

/// Whether `id` can name a connection: it is written to a session row and
/// used as a keychain account, so it is kept to a plain token.
pub fn is_valid_connection_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 40
        && id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'_')
}

/// Check a whole list before it is stored, so a rejected save leaves the
/// previous list intact. Ids are unique and well formed, the URL is http or
/// https, the provider is one zorp speaks, and `api_key_env` is a plain
/// variable name.
pub fn validate_connections(list: &[Connection]) -> Result<(), String> {
    let mut seen = std::collections::HashSet::new();
    for c in list {
        if !is_valid_connection_id(&c.id) {
            return Err(format!(
                "connection id {:?} must be 1 to 40 lowercase letters, digits, - or _",
                c.id
            ));
        }
        if !seen.insert(c.id.as_str()) {
            return Err(format!("connection id {:?} is used twice", c.id));
        }
        let url = c.base_url.trim();
        if !(url.starts_with("http://") || url.starts_with("https://")) {
            return Err(format!(
                "connection {:?}: the base URL must start with http:// or https://",
                c.id
            ));
        }
        if let Some(p) = &c.provider {
            p.parse::<crate::Provider>()
                .map_err(|e| format!("connection {:?}: {e}", c.id))?;
        }
        if let Some(name) = &c.api_key_env {
            let ok = !name.is_empty()
                && name
                    .bytes()
                    .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_');
            if !ok {
                return Err(format!(
                    "connection {:?}: api_key_env must be a variable name like OPENROUTER_API_KEY",
                    c.id
                ));
            }
        }
    }
    Ok(())
}

/// One speed-dial slot: a model a person parked under a number, with an
/// optional label. It carries exactly the non-secret fields a settings save
/// carries and **never an API key**: there is no field for one, so a slot
/// cannot hold a secret even if a caller tried to put one there. Pressing a
/// slot is a settings save of these fields and nothing more; the one API
/// key in `ZORP_API_KEY` is used as it is for the model form.
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
pub struct DialSlot {
    /// 1 to 9. The chord's digit and the slot's identity in the file.
    pub slot: u8,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,
}

impl DialSlot {
    /// Whether the slot points at a usable model. The one check all three
    /// surfaces share (the browser's `slotIsSet`, `/dial`, and `config dial`),
    /// so a slot with a missing or blank model is refused everywhere alike.
    pub fn has_model(&self) -> bool {
        self.model
            .as_deref()
            .map(|m| !m.trim().is_empty())
            .unwrap_or(false)
    }
}

impl Saved {
    /// Apply a speed-dial slot's non-secret model fields over these settings:
    /// the one place the terminal's switch is spelled out, called by both
    /// `config dial` and the REPL's `/dial`. Fields the slot left unset are
    /// left as they are, the same as pressing the chord in the browser. There
    /// is no key to carry, because `DialSlot` has no field for one.
    pub fn apply_dial_slot(&mut self, slot: &DialSlot) {
        if slot.provider.is_some() {
            self.provider = slot.provider.clone();
        }
        if slot.base_url.is_some() {
            self.base_url = slot.base_url.clone();
        }
        if slot.model.is_some() {
            self.model = slot.model.clone();
        }
        if slot.max_tokens.is_some() {
            self.max_tokens = slot.max_tokens;
        }
    }
}

fn non_empty_env(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|s| !s.is_empty())
}

/// The directory the settings file lives in.
///
/// `$XDG_CONFIG_HOME/zorp`, else `$HOME/.config/zorp`, else a local
/// directory so a process with neither still starts. The same shape
/// `trust::state_path` uses for state, one level over in config.
fn config_dir() -> PathBuf {
    non_empty_env("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| non_empty_env("HOME").map(|h| PathBuf::from(h).join(".config")))
        .unwrap_or_else(|| PathBuf::from(".zorp-config"))
        .join("zorp")
}

/// The directory custom emoji live in, `<config>/emoji/`, beside `zorp.toml`.
///
/// A person drops `<name>.{png,gif,webp,svg}` here, or the browser's settings
/// pane writes one. The name is the key a `:name:` in a message looks up; the
/// server scans this directory and never joins a path from a message onto it.
pub fn emoji_dir() -> PathBuf {
    config_dir().join("emoji")
}

/// Where the settings are written.
///
/// `ZORP_CONFIG` wins, then the old `ZORP_WEB_CONFIG` so an existing setup
/// is not broken by the rename, then `zorp.toml` in the config directory.
pub fn path() -> PathBuf {
    if let Some(p) = non_empty_env(PATH_VAR) {
        return PathBuf::from(p);
    }
    if let Some(p) = non_empty_env(LEGACY_PATH_VAR) {
        return PathBuf::from(p);
    }
    config_dir().join(FILE)
}

/// The file an older zorp wrote, read only when the current one is absent.
///
/// The file stopped being the browser's, so it stopped being called
/// `web.toml`. Somebody who configured zorp before that happened should not
/// have to do it again, so the old name is still read. It is never written,
/// so the first `config set` after upgrading moves them onto the new one.
pub fn legacy_path() -> Option<PathBuf> {
    if non_empty_env(PATH_VAR).is_some() || non_empty_env(LEGACY_PATH_VAR).is_some() {
        // An explicit path is an explicit path. Nothing is inferred beside it.
        return None;
    }
    Some(config_dir().join(LEGACY_FILE))
}

/// Read the persisted, non-secret settings.
///
/// A missing file is not an error: nothing has been saved yet, which is the
/// state every install starts in. A corrupt one is reported and ignored
/// rather than stopping a program over a config file.
pub fn load() -> Option<Saved> {
    let current = path();
    if let Some(saved) = read(&current) {
        return Some(saved);
    }
    // Only when the current file is not there at all. A present but empty
    // `zorp.toml` is somebody's answer and does not fall through to a file
    // they may have forgotten about.
    if !current.exists() {
        if let Some(legacy) = legacy_path() {
            return read(&legacy);
        }
    }
    None
}

fn read(path: &std::path::Path) -> Option<Saved> {
    let text = std::fs::read_to_string(path).ok()?;
    match toml::from_str(&text) {
        Ok(saved) => Some(saved),
        Err(e) => {
            eprintln!("zorp: ignoring unreadable {}: {e}", path.display());
            None
        }
    }
}

/// Write the non-secret settings, creating the parent directory if needed.
pub fn save(saved: &Saved) -> std::io::Result<()> {
    let path = path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let text = toml::to_string_pretty(saved)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    std::fs::write(&path, text)
}

/// Where one effective value came from.
///
/// This is the useful half of `zorp-agent config`. A person debugging why
/// they are talking to the wrong model needs the provenance, not the value:
/// they can already see the value, in the wrong answers they are getting.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Source {
    /// A command line flag on this invocation.
    Flag,
    /// An environment variable.
    Env(&'static str),
    /// A flavor manifest, user or project.
    Flavor,
    /// The file this module reads and writes.
    Saved,
    /// Nothing said, so the value is the one compiled in.
    Default,
}

impl Source {
    pub fn describe(self) -> String {
        match self {
            Source::Flag => "a flag on this command".to_string(),
            Source::Env(name) => format!("${name}"),
            Source::Flavor => "a flavor manifest".to_string(),
            Source::Saved => format!("{}", path().display()),
            Source::Default => "the built in default".to_string(),
        }
    }
}

/// One resolved value and where it came from.
#[derive(Clone, Debug, PartialEq)]
pub struct Resolved {
    pub value: String,
    pub source: Source,
}

/// Resolve one setting through the whole chain.
///
/// **The order is flag, environment variable, flavor, saved file, default**,
/// and it is deliberate rather than inherited. A flag is this invocation and
/// beats everything. An environment variable is this shell and beats
/// anything on disk. A flavor is this repository, and it beats the saved
/// file because a project that pins a model means it for that project,
/// where the saved file is a person's standing preference across all of
/// them. The default is what is left.
///
/// The only new step is the saved file, slotted between the flavor and the
/// default, so nothing that used to win stops winning. That was the point:
/// somebody with a working setup should see no change.
pub fn resolve(
    flag: Option<&str>,
    env_var: &'static str,
    flavor: Option<&str>,
    saved: Option<&str>,
    default: &str,
) -> Resolved {
    let usable = |v: &str| !v.trim().is_empty();
    if let Some(v) = flag.filter(|v| usable(v)) {
        return Resolved {
            value: v.to_string(),
            source: Source::Flag,
        };
    }
    if let Some(v) = non_empty_env(env_var).filter(|v| usable(v)) {
        return Resolved {
            value: v,
            source: Source::Env(env_var),
        };
    }
    if let Some(v) = flavor.filter(|v| usable(v)) {
        return Resolved {
            value: v.to_string(),
            source: Source::Flavor,
        };
    }
    if let Some(v) = saved.filter(|v| usable(v)) {
        return Resolved {
            value: v.to_string(),
            source: Source::Saved,
        };
    }
    Resolved {
        value: default.to_string(),
        source: Source::Default,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The environment is process wide, so these take turns and put back
    /// what they found. Without the lock, one test's guard restores while
    /// another is mid-read and `load()` picks up whatever the developer
    /// actually has in `~/.config/zorp`.
    static ENV: std::sync::Mutex<()> = std::sync::Mutex::new(());

    struct EnvGuard(
        Vec<(&'static str, Option<String>)>,
        #[allow(dead_code)] std::sync::MutexGuard<'static, ()>,
    );

    impl EnvGuard {
        fn set(pairs: &[(&'static str, Option<&str>)]) -> EnvGuard {
            let held = ENV.lock().unwrap_or_else(|e| e.into_inner());
            // Every variable this module reads, cleared unless the caller
            // named it, so a test never inherits the developer's own setup.
            const ALL: &[&str] = &[PATH_VAR, LEGACY_PATH_VAR, "XDG_CONFIG_HOME", "HOME"];
            let mut previous: Vec<(&'static str, Option<String>)> = ALL
                .iter()
                .map(|name| (*name, std::env::var(name).ok()))
                .collect();
            for (name, _) in pairs {
                if !ALL.contains(name) {
                    previous.push((*name, std::env::var(name).ok()));
                }
            }
            for name in ALL {
                std::env::remove_var(name);
            }
            for (name, value) in pairs {
                match value {
                    Some(v) => std::env::set_var(name, v),
                    None => std::env::remove_var(name),
                }
            }
            EnvGuard(previous, held)
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            for (name, value) in &self.0 {
                match value {
                    Some(v) => std::env::set_var(name, v),
                    None => std::env::remove_var(name),
                }
            }
        }
    }

    #[test]
    fn the_chain_is_flag_env_flavor_saved_default() {
        let _env = EnvGuard::set(&[("ZORP_MODEL", Some("from-env"))]);

        let all = resolve(
            Some("from-flag"),
            "ZORP_MODEL",
            Some("from-flavor"),
            Some("from-saved"),
            "the-default",
        );
        assert_eq!(all.value, "from-flag");
        assert_eq!(all.source, Source::Flag);

        let no_flag = resolve(
            None,
            "ZORP_MODEL",
            Some("from-flavor"),
            Some("from-saved"),
            "the-default",
        );
        assert_eq!(no_flag.value, "from-env");
        assert_eq!(no_flag.source, Source::Env("ZORP_MODEL"));
    }

    /// A flavor beats the saved file because a project that pins a model
    /// means it for that project, where the file is a standing preference
    /// across all of them.
    #[test]
    fn a_flavor_beats_the_saved_file_and_the_saved_file_beats_the_default() {
        let _env = EnvGuard::set(&[("ZORP_MODEL", None)]);

        let with_flavor = resolve(
            None,
            "ZORP_MODEL",
            Some("from-flavor"),
            Some("from-saved"),
            "the-default",
        );
        assert_eq!(with_flavor.source, Source::Flavor);

        let without = resolve(None, "ZORP_MODEL", None, Some("from-saved"), "the-default");
        assert_eq!(without.value, "from-saved");
        assert_eq!(without.source, Source::Saved);

        let nothing = resolve(None, "ZORP_MODEL", None, None, "the-default");
        assert_eq!(nothing.value, "the-default");
        assert_eq!(nothing.source, Source::Default);
    }

    /// The whole point of adding a step below the flavor: nothing that used
    /// to win stops winning, so an existing setup sees no change.
    #[test]
    fn adding_the_saved_step_changes_nothing_that_was_already_set() {
        let _env = EnvGuard::set(&[("ZORP_MODEL", Some("from-env"))]);

        let before = resolve(None, "ZORP_MODEL", Some("from-flavor"), None, "the-default");
        let after = resolve(
            None,
            "ZORP_MODEL",
            Some("from-flavor"),
            Some("from-saved"),
            "the-default",
        );
        assert_eq!(before, after);
    }

    /// An empty value is nothing said, not a value of "". Otherwise
    /// `ZORP_MODEL=` in a shell profile would silently win over a flavor.
    #[test]
    fn an_empty_value_is_skipped_at_every_step() {
        let _env = EnvGuard::set(&[("ZORP_MODEL", Some(""))]);

        let resolved = resolve(Some("  "), "ZORP_MODEL", Some(""), Some("from-saved"), "d");

        assert_eq!(resolved.value, "from-saved");
        assert_eq!(resolved.source, Source::Saved);
    }

    /// There is nothing on `Saved` to serialize a secret through, and this
    /// is the test that notices if a field is ever added.
    #[test]
    fn no_secret_can_reach_the_file() {
        let saved = Saved {
            provider: Some("openai".to_string()),
            base_url: Some("https://api.openai.com/v1".to_string()),
            model: Some("gpt-4o".to_string()),
            max_tokens: Some(4096),
            workspace: Some("/home/a/work".to_string()),
            speech_model: Some("phonon".to_string()),
            speech_voice: Some("default".to_string()),
            sandbox: Some("workspace".to_string()),
            dial: vec![DialSlot {
                slot: 1,
                label: Some("local".to_string()),
                provider: Some("openai_compatible".to_string()),
                base_url: Some("http://localhost:11434/v1".to_string()),
                model: Some("qwen3:8b".to_string()),
                max_tokens: Some(8192),
            }],
            connections: vec![Connection {
                id: "home".to_string(),
                label: Some("Home Ollama".to_string()),
                provider: Some("openai".to_string()),
                base_url: "http://10.0.0.5:11434/v1".to_string(),
                model: Some("qwen3:32b".to_string()),
                max_tokens: None,
                api_key_env: None,
            }],
            default_connection: Some("home".to_string()),
        };
        let text = toml::to_string_pretty(&saved).unwrap();

        assert!(!text.contains("api_key"), "{text}");
        assert!(!text.contains("key"), "{text}");
        assert!(
            !text.contains("token") || text.contains("max_tokens"),
            "{text}"
        );
        // And a file that somehow carries one does not load it, because
        // there is nowhere for it to go.
        let hostile = format!("{text}\napi_key = \"sk-leaked\"\n");
        let parsed: Saved = toml::from_str(&hostile).unwrap();
        assert_eq!(parsed.model.as_deref(), Some("gpt-4o"));
        assert!(!toml::to_string_pretty(&parsed)
            .unwrap()
            .contains("sk-leaked"));
    }

    /// `has_model` and `apply_dial_slot` are the one copy of the dial switch
    /// that `config dial` and the REPL `/dial` both call, so they agree. A
    /// blank or missing model is not a model, and applying a slot moves only
    /// the fields the slot set.
    #[test]
    fn a_slot_applies_its_set_fields_and_a_blank_model_is_no_model() {
        assert!(!DialSlot {
            slot: 1,
            model: Some("   ".to_string()),
            ..DialSlot::default()
        }
        .has_model());
        assert!(!DialSlot {
            slot: 1,
            ..DialSlot::default()
        }
        .has_model());
        assert!(DialSlot {
            slot: 1,
            model: Some("qwen3:8b".to_string()),
            ..DialSlot::default()
        }
        .has_model());

        let mut saved = Saved {
            model: Some("start".to_string()),
            max_tokens: Some(1024),
            ..Saved::default()
        };
        saved.apply_dial_slot(&DialSlot {
            slot: 2,
            model: Some("next".to_string()),
            base_url: Some("https://x/v1".to_string()),
            ..DialSlot::default()
        });
        assert_eq!(saved.model.as_deref(), Some("next"));
        assert_eq!(saved.base_url.as_deref(), Some("https://x/v1"));
        // The slot left max_tokens unset, so it is unchanged, not cleared.
        assert_eq!(saved.max_tokens, Some(1024));
    }

    #[test]
    fn a_missing_file_is_not_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let _env = EnvGuard::set(&[(
            PATH_VAR,
            Some(dir.path().join("nothing.toml").to_str().unwrap()),
        )]);
        assert_eq!(load(), None);
    }

    #[test]
    fn a_corrupt_file_is_ignored_rather_than_fatal() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("zorp.toml");
        std::fs::write(&path, "this is not toml {{{").unwrap();
        let _env = EnvGuard::set(&[(PATH_VAR, Some(path.to_str().unwrap()))]);

        assert_eq!(load(), None);
    }

    #[test]
    fn what_is_saved_is_what_comes_back() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("zorp.toml");
        let _env = EnvGuard::set(&[(PATH_VAR, Some(path.to_str().unwrap()))]);

        let saved = Saved {
            model: Some("qwen3".to_string()),
            base_url: Some("http://localhost:11434/v1".to_string()),
            ..Saved::default()
        };
        save(&saved).unwrap();

        assert_eq!(load(), Some(saved));
    }

    /// Somebody who configured zorp before the file was shared should not
    /// have to do it again.
    #[test]
    fn the_old_web_toml_is_still_read_when_there_is_no_new_one() {
        let dir = tempfile::tempdir().unwrap();
        let config = dir.path().join("zorp");
        std::fs::create_dir_all(&config).unwrap();
        std::fs::write(config.join("web.toml"), "model = \"from-web-toml\"\n").unwrap();
        let _env = EnvGuard::set(&[
            (PATH_VAR, None),
            (LEGACY_PATH_VAR, None),
            ("XDG_CONFIG_HOME", Some(dir.path().to_str().unwrap())),
        ]);

        assert_eq!(
            load().and_then(|s| s.model).as_deref(),
            Some("from-web-toml")
        );

        // And the new name wins the moment it exists.
        std::fs::write(config.join("zorp.toml"), "model = \"from-zorp-toml\"\n").unwrap();
        assert_eq!(
            load().and_then(|s| s.model).as_deref(),
            Some("from-zorp-toml")
        );
    }

    /// An explicit path is an explicit path. Nothing is inferred beside it,
    /// or a test fixture would quietly pick up the developer's own file.
    #[test]
    fn an_explicit_path_does_not_fall_through_to_the_old_name() {
        let dir = tempfile::tempdir().unwrap();
        let config = dir.path().join("zorp");
        std::fs::create_dir_all(&config).unwrap();
        std::fs::write(config.join("web.toml"), "model = \"from-web-toml\"\n").unwrap();
        let _env = EnvGuard::set(&[
            (
                PATH_VAR,
                Some(dir.path().join("elsewhere.toml").to_str().unwrap()),
            ),
            ("XDG_CONFIG_HOME", Some(dir.path().to_str().unwrap())),
        ]);

        assert_eq!(load(), None);
    }
}
