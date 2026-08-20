use crate::approval::WebApprover;
use crate::event::Event;
use std::collections::HashMap;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};

/// Everything one conversation needs while it is alive.
///
/// The backlog is the whole event history for the session, which is what makes
/// reconnection cheap: a client sends the last seq it saw and the stream
/// replays from there.
///
/// There is deliberately no "is this session finished" flag. There used to be
/// one, and because the backlog is never cleared it read as finished forever
/// after the first turn, which ended every later event stream the moment it
/// opened. A session is finished when it is gone. A turn is finished when a
/// `Done` event goes out, and that is the client's business.
pub struct SessionState {
    pub backlog: Vec<Event>,
    pub running: bool,
    pub approver: Option<Arc<WebApprover>>,
    /// Sequence counter for the whole session, not one turn.
    ///
    /// Last-Event-ID resume is keyed on this, so restarting it per turn makes
    /// a reconnecting browser silently drop every later turn.
    pub seq: Arc<Mutex<u64>>,
    /// The session's standing answer to every approval it is asked.
    ///
    /// False unless this browser turned it on for this session, and it is
    /// deliberately not written anywhere: it lives here, in memory, beside a
    /// session that only exists while the server does. There is no config file
    /// and no database column for it, so no future session can inherit it and
    /// no restart can bring it back. Turning the approval gate down is a thing
    /// you do to the run in front of you, not a preference.
    ///
    /// Shared with each turn's `WebApprover` rather than copied into it, which
    /// is what makes it revocable while a turn is running and what carries it
    /// from one turn to the next in the same conversation.
    pub auto_approve: Arc<AtomicBool>,
}

impl SessionState {
    fn new() -> Self {
        SessionState {
            backlog: Vec::new(),
            running: false,
            approver: None,
            seq: Arc::new(Mutex::new(0)),
            auto_approve: Arc::new(AtomicBool::new(false)),
        }
    }
}

/// Shared handle to the model settings, following the same `Arc<Mutex<..>>`
/// pattern as `sessions`. Cloned into `turn::spawn_turn` so a running turn
/// resolves against the same state a concurrent `GET /api/settings` sees.
pub type SettingsHandle = Arc<Mutex<crate::settings::SettingsState>>;

#[derive(Clone, Default)]
pub struct AppState {
    sessions: Arc<Mutex<HashMap<String, Arc<Mutex<SessionState>>>>>,
    /// Shared secret, set only when the server binds a non-loopback
    /// interface. None means the operating system is the access control.
    pub token: Option<String>,
    /// Model settings: a UI-saved value, the live env vars, and the
    /// hardcoded defaults, in that precedence order. Seeded at construction
    /// with `SettingsState::seeded_from_env`, which only captures
    /// `ZORP_API_KEY` (see its doc comment for why); loading a persisted
    /// config file on top is `main.rs`'s job, not this constructor's, so
    /// that tests built on `AppState::new`/`with_token` never depend on
    /// whatever the developer machine happens to have saved.
    pub settings: SettingsHandle,
    /// The directory artifacts are served from, and the boundary nothing
    /// served may escape. The agent already works in this directory, so this
    /// adds no reach; what it adds is a second way to read from it, which is
    /// why `artifacts.rs` resolves every request against this and refuses
    /// anything that lands outside. `None` turns the artifact endpoints off
    /// entirely rather than defaulting to somewhere surprising.
    pub workspace: Option<std::path::PathBuf>,
}

impl AppState {
    pub fn new() -> Self {
        AppState {
            settings: Arc::new(Mutex::new(crate::settings::SettingsState::seeded_from_env())),
            ..AppState::default()
        }
    }

    pub fn with_token(token: Option<String>) -> Self {
        AppState {
            token,
            settings: Arc::new(Mutex::new(crate::settings::SettingsState::seeded_from_env())),
            ..AppState::default()
        }
    }

    /// Point the artifact endpoints at a directory. `main.rs` passes the
    /// directory the server was started in, which is the one the agent
    /// works in.
    pub fn with_workspace(mut self, root: std::path::PathBuf) -> Self {
        self.workspace = Some(root);
        self
    }

    pub fn create(&self, id: &str) -> Arc<Mutex<SessionState>> {
        let session = Arc::new(Mutex::new(SessionState::new()));
        self.sessions
            .lock()
            .unwrap()
            .insert(id.to_string(), Arc::clone(&session));
        session
    }

    pub fn get(&self, id: &str) -> Option<Arc<Mutex<SessionState>>> {
        self.sessions.lock().unwrap().get(id).map(Arc::clone)
    }

    pub fn ids(&self) -> Vec<String> {
        self.sessions.lock().unwrap().keys().cloned().collect()
    }
}
