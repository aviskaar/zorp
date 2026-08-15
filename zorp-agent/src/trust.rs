use std::collections::BTreeSet;
use std::io::Write;
use std::path::PathBuf;

/// Remembers approved project-flavor content hashes, one lowercase-hex hash per
/// line in a small state file. Read errors degrade to "not trusted"; write
/// errors are surfaced so a failed persist is not mistaken for success.
pub struct TrustStore {
    path: PathBuf,
    hashes: BTreeSet<String>,
}

/// Resolve a zorp state file path: `$<env_var>` if set (and non-empty),
/// otherwise `$XDG_STATE_HOME/zorp/<leaf>` falling back to
/// `$HOME/.local/state/zorp/<leaf>`, and finally `.zorp-state/zorp/<leaf>`
/// if neither is set. Shared by `TrustStore::default_path` and
/// `Store::default_path`, which differ only in env var and leaf name.
pub(crate) fn state_path(env_var: &str, leaf: &str) -> PathBuf {
    if let Ok(p) = std::env::var(env_var) {
        if !p.is_empty() {
            return PathBuf::from(p);
        }
    }
    let base = std::env::var("XDG_STATE_HOME")
        .ok()
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var("HOME")
                .ok()
                .map(|h| PathBuf::from(h).join(".local/state"))
        })
        .unwrap_or_else(|| PathBuf::from(".zorp-state"));
    base.join("zorp").join(leaf)
}

impl TrustStore {
    pub fn default_path() -> PathBuf {
        state_path("ZORP_TRUST_FILE", "trust")
    }

    pub fn open() -> TrustStore {
        TrustStore::open_at(TrustStore::default_path())
    }

    pub fn open_at(path: PathBuf) -> TrustStore {
        let hashes = std::fs::read_to_string(&path)
            .map(|text| {
                text.lines()
                    .map(str::trim)
                    .filter(|l| !l.is_empty())
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default();
        TrustStore { path, hashes }
    }

    pub fn is_trusted(&self, hash: &str) -> bool {
        self.hashes.contains(hash)
    }

    /// Record a hash as trusted and persist the store. The write goes to a
    /// temp file in the same directory (mode 0600 on unix) and is renamed
    /// into place, so a failed write never truncates the existing store.
    pub fn trust(&mut self, hash: &str) -> std::io::Result<()> {
        if !self.hashes.insert(hash.to_string()) {
            return Ok(());
        }
        if let Some(parent) = self.path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent)?;
            }
        }
        let body: String = self.hashes.iter().cloned().collect::<Vec<_>>().join("\n");
        atomic_write(&self.path, format!("{body}\n").as_bytes())
    }
}

/// Write `contents` to `path` via a temp file in the same directory, created
/// with owner-only permissions on unix, then rename it into place.
pub(crate) fn atomic_write(path: &std::path::Path, contents: &[u8]) -> std::io::Result<()> {
    let file_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("state");
    let tmp = path.with_file_name(format!(".{file_name}.tmp{}", std::process::id()));
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let result = options
        .open(&tmp)
        .and_then(|mut file| file.write_all(contents))
        .and_then(|_| std::fs::rename(&tmp, path));
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trust_persists_across_reopen() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("trust");
        let mut store = TrustStore::open_at(path.clone());
        assert!(!store.is_trusted("abc"));
        store.trust("abc").unwrap();
        assert!(store.is_trusted("abc"));
        // Reopen: the hash is still there.
        let reopened = TrustStore::open_at(path);
        assert!(reopened.is_trusted("abc"));
        assert!(!reopened.is_trusted("def"));
    }

    #[test]
    fn missing_file_is_empty_not_error() {
        let dir = tempfile::tempdir().unwrap();
        let store = TrustStore::open_at(dir.path().join("nope"));
        assert!(!store.is_trusted("x"));
    }

    #[cfg(unix)]
    #[test]
    fn trust_write_failure_is_surfaced() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let readonly = dir.path().join("locked");
        std::fs::create_dir(&readonly).unwrap();
        std::fs::set_permissions(&readonly, std::fs::Permissions::from_mode(0o500)).unwrap();

        let mut store = TrustStore::open_at(readonly.join("trust"));
        let result = store.trust("abc");

        std::fs::set_permissions(&readonly, std::fs::Permissions::from_mode(0o700)).unwrap();
        assert!(result.is_err(), "write into a read-only dir must error");
    }

    #[cfg(unix)]
    #[test]
    fn trust_file_is_owner_only() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("trust");
        let mut store = TrustStore::open_at(path.clone());
        store.trust("abc").unwrap();

        let mode = std::fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
    }

    #[test]
    fn trust_leaves_no_temp_file_behind() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = TrustStore::open_at(dir.path().join("trust"));
        store.trust("abc").unwrap();
        let names: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names, vec!["trust".to_string()]);
    }
}
