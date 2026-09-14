//! What zorp keeps on this machine, and how to get rid of it.
//!
//! Five files, written by four different parts of the program, and until
//! now the only way to find out what was in them was to know where to look.
//! This is the one list, so a person can see what is held and clear it, and
//! so the browser and the terminal agree about what "reset" means.
//!
//! It lives in `zorp-agent` rather than in `zorp-web` for the same reason
//! the session store does: both surfaces keep the same state, in the same
//! place, and a second implementation of "delete everything" is a second
//! chance to delete the wrong thing.
//!
//! Two rules hold across everything here.
//!
//! **Nothing in this module touches a workspace file.** `<workspace>/scratch`
//! and everything beside it belong to the person. A reset that reached into
//! them would be the one action nobody could undo, and there is no code path
//! here that can.
//!
//! **Nothing here is reachable by a model.** No tool clears data, and
//! `zorp-agent/src/agent.rs` has a test naming this module that says so.
//! Deleting is a person's decision every time.

use std::path::{Path, PathBuf};

/// One file zorp keeps, and what it is for.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StateFile {
    /// What a person calls it.
    pub label: &'static str,
    /// One sentence saying what is in it and what losing it costs.
    pub what: &'static str,
    pub path: PathBuf,
    /// The variable that moves it, when there is one. Worth showing: a
    /// person can easily be looking at a different database than they think.
    pub env_var: Option<&'static str>,
    /// Size on disk, or `None` when the file is not there. Absent and empty
    /// are different answers and a listing that conflated them would send
    /// somebody looking for a file that was never written.
    pub bytes: Option<u64>,
}

impl StateFile {
    fn new(
        label: &'static str,
        what: &'static str,
        path: PathBuf,
        env_var: Option<&'static str>,
    ) -> Self {
        let bytes = std::fs::metadata(&path).ok().map(|m| m.len());
        StateFile {
            label,
            what,
            path,
            env_var,
            bytes,
        }
    }

    pub fn exists(&self) -> bool {
        self.bytes.is_some()
    }
}

/// Where the conversation search index lives.
///
/// The same answer `recall::index_path` gives, derived here so it can be
/// reported and deleted in a build with no `recall` feature. A default
/// build still has the file if it was once built with the feature on, and a
/// listing that could not see it would be telling somebody they hold
/// nothing while a database sits beside the session store.
pub fn recall_index_path() -> PathBuf {
    if let Ok(p) = std::env::var("ZORP_RECALL_DB") {
        if !p.is_empty() {
            return PathBuf::from(p);
        }
    }
    let sessions = crate::session::Store::default_path();
    match sessions.parent() {
        Some(dir) if !dir.as_os_str().is_empty() => dir.join("recall.db"),
        _ => PathBuf::from("recall.db"),
    }
}

/// Every file zorp keeps, in the order a person would read them.
///
/// Resolved through the same functions the code that writes them uses, so
/// this reports the files that would actually be used rather than the ones
/// a default install would have had.
pub fn files() -> Vec<StateFile> {
    vec![
        StateFile::new(
            "conversations",
            "every conversation both surfaces have had, with its messages and file changes",
            crate::session::Store::default_path(),
            Some("ZORP_STATE_DB"),
        ),
        StateFile::new(
            "search index",
            "embeddings of past conversations, for recall. Rebuilt from the conversations above",
            recall_index_path(),
            Some("ZORP_RECALL_DB"),
        ),
        StateFile::new(
            "input history",
            "what was typed at the chat prompt, so the up arrow survives a restart",
            crate::line_editor::history_path(),
            Some("ZORP_HISTORY_FILE"),
        ),
        StateFile::new(
            "trusted agents",
            "content hashes of project flavors a person approved. Clearing it makes them untrusted again, not broken",
            crate::trust::TrustStore::default_path(),
            Some("ZORP_TRUST_FILE"),
        ),
        StateFile::new(
            "settings",
            "the saved model, endpoint and workspace. Never the API key, which is only ever in the environment",
            crate::config::path(),
            Some(crate::config::PATH_VAR),
        ),
    ]
}

/// What one clearing action removed.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Cleared {
    /// Files actually removed. A file that was not there is not an error
    /// and is not listed, because "cleared something that did not exist" is
    /// not a thing that happened.
    pub removed: Vec<PathBuf>,
    /// Rows removed, when the action was about rows rather than files.
    pub rows: usize,
}

impl Cleared {
    pub fn is_empty(&self) -> bool {
        self.removed.is_empty() && self.rows == 0
    }
}

/// Remove a file, treating "it was not there" as success.
///
/// The distinction matters for the report and not for the outcome: either
/// way the file is gone afterwards, which is what was asked for.
fn remove(path: &Path, into: &mut Vec<PathBuf>) -> std::io::Result<()> {
    match std::fs::remove_file(path) {
        Ok(()) => {
            into.push(path.to_path_buf());
            Ok(())
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e),
    }
}

/// Delete the search index.
///
/// Safe in a way the other two are not: the index is derived from the
/// conversations, so the next sweep rebuilds it and nothing is lost that
/// cannot be recomputed. It is here because it can get large and because a
/// person who has just cleared their conversations should not be left with
/// embeddings of them.
pub fn delete_search_index() -> std::io::Result<Cleared> {
    let mut cleared = Cleared::default();
    let index = recall_index_path();
    remove(&index, &mut cleared.removed)?;
    // SQLite's journal and write-ahead files sit beside the database under
    // derived names. Leaving them would leave a partial index behind that
    // the next open would try to replay.
    for suffix in ["-journal", "-wal", "-shm"] {
        let mut name = index.clone().into_os_string();
        name.push(suffix);
        remove(Path::new(&name), &mut cleared.removed)?;
    }
    Ok(cleared)
}

/// Put the settings back to what the environment alone would give.
///
/// Removes the settings file and the trust file. Not the conversations, and
/// not anything in a workspace.
///
/// **It cannot clear `ZORP_API_KEY` and says so rather than implying
/// otherwise.** A process does not own the environment it was started in,
/// so a key exported in the shell that launched zorp survives this and
/// survives a restart of the server. That sentence belongs in front of
/// whoever clicks the button, which is why it is a constant here rather
/// than a comment.
pub const RESET_LEAVES_THE_KEY: &str =
    "ZORP_API_KEY is read from the environment this process was started in, so \
     resetting settings cannot unset it. Close the shell or unset it there.";

pub fn reset_settings() -> std::io::Result<Cleared> {
    let mut cleared = Cleared::default();
    remove(&crate::config::path(), &mut cleared.removed)?;
    // The old `web.toml`, which `config::load` still reads when the current
    // file is absent. Removing only the current one is a reset that does
    // not reset: on the next read the pre-rename settings come back, and
    // the surface that said it had cleared them shows them again.
    if let Some(legacy) = crate::config::legacy_path() {
        remove(&legacy, &mut cleared.removed)?;
    }
    remove(
        &crate::trust::TrustStore::default_path(),
        &mut cleared.removed,
    )?;
    Ok(cleared)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// The paths come from process-wide environment, so these take turns.
    static ENV: Mutex<()> = Mutex::new(());

    /// Serialise, and do not let one failure poison the rest of the file.
    fn lock() -> std::sync::MutexGuard<'static, ()> {
        ENV.lock().unwrap_or_else(|e| e.into_inner())
    }

    struct Guard {
        dir: tempfile::TempDir,
    }

    impl Guard {
        fn new() -> Guard {
            let dir = tempfile::tempdir().unwrap();
            std::env::set_var("ZORP_STATE_DB", dir.path().join("sessions.db"));
            std::env::set_var("ZORP_RECALL_DB", dir.path().join("recall.db"));
            std::env::set_var("ZORP_HISTORY_FILE", dir.path().join("history"));
            std::env::set_var("ZORP_TRUST_FILE", dir.path().join("trust"));
            std::env::set_var("ZORP_CONFIG", dir.path().join("zorp.toml"));
            Guard { dir }
        }

        fn write(&self, leaf: &str, body: &str) -> PathBuf {
            let path = self.dir.path().join(leaf);
            std::fs::write(&path, body).unwrap();
            path
        }
    }

    impl Drop for Guard {
        fn drop(&mut self) {
            for var in [
                "ZORP_STATE_DB",
                "ZORP_RECALL_DB",
                "ZORP_HISTORY_FILE",
                "ZORP_TRUST_FILE",
                "ZORP_CONFIG",
            ] {
                std::env::remove_var(var);
            }
        }
    }

    #[test]
    fn every_file_is_listed_with_where_it_is_and_what_it_holds() {
        let _lock = lock();
        let guard = Guard::new();
        guard.write("sessions.db", "not really a database");

        let files = files();
        assert_eq!(files.len(), 5, "{files:?}");
        for file in &files {
            assert!(!file.what.is_empty(), "{} has no explanation", file.label);
            assert!(file.path.is_absolute(), "{} is not absolute", file.label);
        }
        let conversations = files.iter().find(|f| f.label == "conversations").unwrap();
        assert!(conversations.exists());
        assert_eq!(
            conversations.bytes,
            Some("not really a database".len() as u64)
        );
        assert_eq!(conversations.env_var, Some("ZORP_STATE_DB"));
    }

    /// Absent and empty are different answers. A listing that showed a file
    /// that was never written as zero bytes would send somebody looking for
    /// it.
    #[test]
    fn a_file_that_is_not_there_is_absent_rather_than_zero() {
        let _lock = lock();
        let guard = Guard::new();
        guard.write("history", "");

        let files = files();
        let history = files.iter().find(|f| f.label == "input history").unwrap();
        assert_eq!(history.bytes, Some(0));
        assert!(history.exists());

        let index = files.iter().find(|f| f.label == "search index").unwrap();
        assert_eq!(index.bytes, None);
        assert!(!index.exists());
    }

    #[test]
    fn deleting_the_index_takes_its_journal_files_with_it() {
        let _lock = lock();
        let guard = Guard::new();
        guard.write("recall.db", "index");
        guard.write("recall.db-wal", "ahead");
        guard.write("recall.db-shm", "shared");

        let cleared = delete_search_index().unwrap();
        assert_eq!(cleared.removed.len(), 3, "{cleared:?}");
        assert!(!guard.dir.path().join("recall.db").exists());
        assert!(!guard.dir.path().join("recall.db-wal").exists());
    }

    #[test]
    fn deleting_an_index_that_is_not_there_is_not_an_error() {
        let _lock = lock();
        let _guard = Guard::new();
        let cleared = delete_search_index().unwrap();
        assert!(cleared.is_empty(), "{cleared:?}");
    }

    /// The load bearing one. Reset takes the settings and the trust file and
    /// nothing else, and in particular it does not take the conversations,
    /// which are the thing somebody would most regret losing to a button
    /// labelled "reset settings".
    #[test]
    fn resetting_settings_leaves_the_conversations_alone() {
        let _lock = lock();
        let guard = Guard::new();
        guard.write("zorp.toml", "model = \"m\"");
        guard.write("trust", "abc123");
        let sessions = guard.write("sessions.db", "conversations");
        let history = guard.write("history", "what was typed");

        let cleared = reset_settings().unwrap();
        assert_eq!(cleared.removed.len(), 2, "{cleared:?}");
        assert!(sessions.exists(), "reset deleted the conversations");
        assert!(history.exists(), "reset deleted the input history");
    }

    /// Reset has to take the file an older zorp wrote, or it does not reset.
    ///
    /// `config::load` reads `web.toml` when `zorp.toml` is absent, which is
    /// what stops a pre-rename setup breaking on upgrade. Removing only the
    /// current file therefore leaves the old settings live: the next read
    /// finds them, and the surface that said it had cleared them shows them
    /// again with no way to tell why.
    #[test]
    fn reset_takes_the_pre_rename_settings_file_too() {
        let _lock = lock();
        let guard = Guard::new();
        // `legacy_path` is inferred only when no explicit path is set, so
        // the inferred config directory is what this has to exercise.
        std::env::remove_var("ZORP_CONFIG");
        std::env::remove_var("ZORP_WEB_CONFIG");
        let previous = std::env::var("XDG_CONFIG_HOME").ok();
        std::env::set_var("XDG_CONFIG_HOME", guard.dir.path());

        let legacy = crate::config::legacy_path().expect("inferred, so there is one");
        std::fs::create_dir_all(legacy.parent().unwrap()).unwrap();
        std::fs::write(&legacy, "model = \"from-web-toml\"\n").unwrap();

        let cleared = reset_settings();

        match previous {
            Some(p) => std::env::set_var("XDG_CONFIG_HOME", p),
            None => std::env::remove_var("XDG_CONFIG_HOME"),
        }
        cleared.unwrap();

        assert!(
            !legacy.exists(),
            "the old settings survived a reset and would be read again"
        );
    }

    /// Said out loud, because the alternative is a person clicking reset and
    /// believing their key is gone when the environment still holds it.
    #[test]
    fn the_note_about_the_key_says_it_cannot_be_unset_here() {
        assert!(RESET_LEAVES_THE_KEY.contains("ZORP_API_KEY"));
        assert!(RESET_LEAVES_THE_KEY.contains("cannot"));
    }
}
