//! The directory the agent works in, chosen by a person.
//!
//! `zorp-web` used to run the agent in the directory the server was started
//! in, which in practice is the zorp checkout. Every file the agent wrote,
//! every PDF it rendered and every throwaway script it left behind landed in
//! zorp's own source tree. So the working directory is now a thing somebody
//! picks: `--workspace`, then `ZORP_WORKSPACE`, then whatever was last saved
//! through the browser.
//!
//! There is deliberately no fourth candidate. Falling back to the current
//! directory is the bug, not the safety net: a server that guesses is a
//! server that writes somewhere nobody chose, and the guess is invisible
//! until the files show up. With nothing chosen the server still starts and
//! still serves the UI, and refuses to run work until somebody picks a
//! directory.
//!
//! See `docs/DECISIONS.md` (2026-09-05).

use serde::Serialize;
use std::path::{Path, PathBuf};

/// The body every refusal to run work carries when no workspace is chosen.
///
/// Exact text, and the status beside it is 409. The browser matches on both,
/// so it can offer the directory picker instead of showing an error.
pub const NO_WORKSPACE: &str = "no workspace chosen";

/// The environment variable that names a workspace.
pub const ENV_VAR: &str = "ZORP_WORKSPACE";

/// Generated files go here, under the workspace.
pub const SCRATCH: &str = "scratch";

/// What named the effective workspace, so the browser can say "from
/// ZORP_WORKSPACE" rather than implying somebody chose it in the UI.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Source {
    Flag,
    Env,
    Saved,
    None,
}

impl Source {
    /// How to name this source to a person reading stderr.
    pub fn describe(self) -> &'static str {
        match self {
            Source::Flag => "--workspace",
            Source::Env => ENV_VAR,
            Source::Saved => "the settings file",
            Source::None => "nothing",
        }
    }
}

/// A usable workspace: an absolute, canonical directory, and what named it.
#[derive(Clone, Debug)]
pub struct Chosen {
    pub path: PathBuf,
    pub source: Source,
}

impl Chosen {
    /// Where generated files belong.
    pub fn scratch(&self) -> PathBuf {
        self.path.join(SCRATCH)
    }
}

/// Why there is no workspace to work in.
#[derive(Debug)]
pub enum Unusable {
    /// Nobody has chosen one.
    Unset,
    /// One was named and cannot be used. The sentence says why.
    Refused { source: Source, reason: String },
}

impl Unusable {
    /// What named the path that cannot be used, or `None` when nothing did.
    pub fn source(&self) -> Source {
        match self {
            Unusable::Unset => Source::None,
            Unusable::Refused { source, .. } => *source,
        }
    }
}

/// The effective workspace, or why there is not one.
///
/// The first candidate that exists wins, and then it has to pass. A named
/// path that no longer validates does not fall through to the next
/// candidate: somebody said to work there, and quietly working somewhere
/// else instead is the same failure as guessing.
pub fn resolve(flag: Option<&Path>, saved: Option<&str>) -> Result<Chosen, Unusable> {
    let (named, source) = named(flag, saved).ok_or(Unusable::Unset)?;
    match validate(&named) {
        Ok(path) => Ok(Chosen { path, source }),
        Err(reason) => Err(Unusable::Refused { source, reason }),
    }
}

/// Which path was named, and by what.
fn named(flag: Option<&Path>, saved: Option<&str>) -> Option<(PathBuf, Source)> {
    if let Some(path) = flag {
        return Some((path.to_path_buf(), Source::Flag));
    }
    if let Some(path) = std::env::var(ENV_VAR).ok().filter(|s| !s.trim().is_empty()) {
        return Some((PathBuf::from(path.trim()), Source::Env));
    }
    let saved = saved.map(str::trim).filter(|s| !s.is_empty())?;
    Some((PathBuf::from(saved), Source::Saved))
}

/// Check a path the same way wherever it came from.
///
/// The flag, the environment variable, the saved value and the path a person
/// just typed into the browser all come through here, so a path refused in
/// one place is refused in all of them. The error is a sentence somebody can
/// act on, because it is shown to them.
pub fn validate(path: &Path) -> Result<PathBuf, String> {
    if path.as_os_str().is_empty() {
        return Err("no path given".to_string());
    }
    if !path.is_absolute() {
        return Err(format!(
            "{} is not an absolute path, so give the whole path starting from /",
            path.display()
        ));
    }
    // Canonicalizing is the existence check as well: a path that is not
    // there cannot be resolved. It also settles symlinks now rather than on
    // every later read.
    let canonical = path
        .canonicalize()
        .map_err(|e| format!("{} cannot be opened: {e}", path.display()))?;
    if !canonical.is_dir() {
        return Err(format!(
            "{} is not a directory, so it cannot be a workspace",
            canonical.display()
        ));
    }
    if canonical.parent().is_none() {
        return Err(format!(
            "{} is the whole filesystem, so pick a directory under it instead",
            canonical.display()
        ));
    }
    // Readable, not merely present. A directory the server cannot open is a
    // workspace every turn would fail in, and finding that out now is worth
    // one call.
    std::fs::read_dir(&canonical)
        .map_err(|e| format!("{} cannot be read: {e}", canonical.display()))?;
    Ok(canonical)
}

/// Make `<workspace>/scratch`, and say so rather than fail when it cannot be
/// made.
///
/// Called when a turn starts and not at startup, because a workspace can be
/// chosen while the server is running. A turn whose scratch directory could
/// not be created is still a turn worth running: the model is told where
/// generated files go, and a missing directory is a thing it can create
/// itself or work around.
pub fn ensure_scratch(root: &Path) -> Option<PathBuf> {
    let scratch = root.join(SCRATCH);
    match std::fs::create_dir_all(&scratch) {
        Ok(()) => Some(scratch),
        Err(e) => {
            eprintln!("zorp-web: could not create {}: {e}", scratch.display());
            None
        }
    }
}

/// One directory in a browse listing.
#[derive(Debug, Serialize)]
pub struct Entry {
    pub name: String,
    pub path: String,
}

/// What `GET /api/workspace/browse` answers with.
#[derive(Debug, Serialize)]
pub struct Listing {
    pub path: String,
    pub parent: Option<String>,
    pub entries: Vec<Entry>,
}

/// Why a browse request was refused, and with what status.
#[derive(Debug)]
pub enum BrowseError {
    /// The caller sent something other than an absolute path.
    NotAbsolute(String),
    /// There is no such directory.
    Missing(String),
    /// It is there and this server cannot read it.
    Denied(String),
}

/// List the subdirectories of one directory, so a person can pick a
/// workspace without typing a path.
///
/// This lists directory names on the machine the server runs on, and that is
/// all it does. It never returns a file name, never any file's contents, and
/// never walks anywhere except the one directory it was asked for. It is
/// exactly as exposed as the shell the agent already runs on this machine,
/// which is why `zorp-web` refuses a non-loopback bind without a token: a
/// reachable server was already agent-driven shell access, and this endpoint
/// adds no reach that access did not have.
pub fn browse(path: Option<&str>, hidden: bool) -> Result<Listing, BrowseError> {
    let asked = path.map(str::trim).filter(|s| !s.is_empty());
    let root = match asked {
        Some(p) => {
            let p = PathBuf::from(p);
            if !p.is_absolute() {
                return Err(BrowseError::NotAbsolute(format!(
                    "{} is not an absolute path, so give the whole path starting from /",
                    p.display()
                )));
            }
            p
        }
        // No path is the first question a picker asks, and home is the
        // answer nearly everybody wants.
        None => home(),
    };

    let mut entries = Vec::new();
    for entry in std::fs::read_dir(&root).map_err(|e| browse_error(&root, e))? {
        let Ok(entry) = entry else { continue };
        let name = entry.file_name().to_string_lossy().into_owned();
        if !hidden && name.starts_with('.') {
            continue;
        }
        // `is_dir` follows the link, which is what lists a symlinked
        // directory as the directory it points at.
        if !entry.path().is_dir() {
            continue;
        }
        entries.push(Entry {
            path: entry.path().to_string_lossy().into_owned(),
            name,
        });
    }
    // Case insensitive, because a picker sorted by byte value puts every
    // capitalized name above every lowercase one.
    entries.sort_by_key(|e| e.name.to_lowercase());

    Ok(Listing {
        parent: root.parent().map(|p| p.to_string_lossy().into_owned()),
        path: root.to_string_lossy().into_owned(),
        entries,
    })
}

fn browse_error(root: &Path, e: std::io::Error) -> BrowseError {
    match e.kind() {
        // A path that is not there and a path that is a file are the same
        // answer to a picker: there is no directory to list here.
        std::io::ErrorKind::NotFound | std::io::ErrorKind::NotADirectory => {
            BrowseError::Missing(format!("there is no directory at {}", root.display()))
        }
        _ => BrowseError::Denied(format!("{} cannot be read: {e}", root.display())),
    }
}

/// The directory a picker opens in. `HOME`, the same variable
/// `settings::config_path` reads, and the filesystem root when there is none.
fn home() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_relative_path_is_refused_with_a_sentence() {
        let err = validate(Path::new("some/dir")).unwrap_err();
        assert!(err.contains("absolute"), "{err}");
    }

    #[test]
    fn a_path_that_is_not_there_is_refused() {
        let err = validate(Path::new("/no/such/directory/anywhere")).unwrap_err();
        assert!(err.contains("cannot be opened"), "{err}");
    }

    #[test]
    fn a_file_is_not_a_workspace() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("notes.md");
        std::fs::write(&file, "hi").unwrap();
        let err = validate(&file).unwrap_err();
        assert!(err.contains("not a directory"), "{err}");
    }

    /// Everything is under the root, so accepting it would put the agent's
    /// scratch directory beside /etc.
    #[test]
    fn the_filesystem_root_is_refused() {
        let err = validate(Path::new("/")).unwrap_err();
        assert!(err.contains("whole filesystem"), "{err}");
    }

    #[test]
    fn an_ordinary_directory_comes_back_canonical() {
        let dir = tempfile::tempdir().unwrap();
        let ok = validate(dir.path()).unwrap();
        assert_eq!(ok, dir.path().canonicalize().unwrap());
    }

    #[test]
    fn scratch_sits_under_the_workspace() {
        let chosen = Chosen {
            path: PathBuf::from("/tmp/work"),
            source: Source::Flag,
        };
        assert_eq!(chosen.scratch(), PathBuf::from("/tmp/work/scratch"));
    }

    /// Directories only, and no dotted ones unless they were asked for.
    #[test]
    fn browse_lists_subdirectories_and_never_files() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("papers")).unwrap();
        std::fs::create_dir(dir.path().join(".hidden")).unwrap();
        std::fs::write(dir.path().join("notes.md"), "hi").unwrap();

        let listing = browse(Some(&dir.path().to_string_lossy()), false).unwrap();
        let names: Vec<&str> = listing.entries.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, vec!["papers"]);

        let with_hidden = browse(Some(&dir.path().to_string_lossy()), true).unwrap();
        let names: Vec<&str> = with_hidden
            .entries
            .iter()
            .map(|e| e.name.as_str())
            .collect();
        assert_eq!(names, vec![".hidden", "papers"]);
    }

    /// A symlinked directory is a directory. Somebody whose projects live
    /// behind one should be able to pick it.
    #[cfg(unix)]
    #[test]
    fn browse_lists_a_symlinked_directory() {
        let dir = tempfile::tempdir().unwrap();
        let real = dir.path().join("real");
        std::fs::create_dir(&real).unwrap();
        std::os::unix::fs::symlink(&real, dir.path().join("linked")).unwrap();
        let listing = browse(Some(&dir.path().to_string_lossy()), false).unwrap();
        let names: Vec<&str> = listing.entries.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, vec!["linked", "real"]);
    }

    #[test]
    fn browse_sorts_without_regard_to_case() {
        let dir = tempfile::tempdir().unwrap();
        for name in ["Zebra", "apple", "Banana"] {
            std::fs::create_dir(dir.path().join(name)).unwrap();
        }
        let listing = browse(Some(&dir.path().to_string_lossy()), false).unwrap();
        let names: Vec<&str> = listing.entries.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, vec!["apple", "Banana", "Zebra"]);
    }

    #[test]
    fn browse_refuses_a_relative_path_and_reports_a_missing_one() {
        assert!(matches!(
            browse(Some("relative/dir"), false),
            Err(BrowseError::NotAbsolute(_))
        ));
        assert!(matches!(
            browse(Some("/no/such/directory/anywhere"), false),
            Err(BrowseError::Missing(_))
        ));
    }
}
