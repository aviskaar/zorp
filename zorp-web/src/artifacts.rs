//! Serving files the agent produced, to a browser, without serving anything
//! else. See `docs/superpowers/specs/2026-08-17-artifact-pane-design.md`.
//!
//! `zorp-web` already runs commands and edits files in the directory it was
//! started in, so reading from that same directory is not new reach. What is
//! new is a second door into it, one that takes a path from a URL. Every
//! rule in this module exists because of that door.

use std::path::{Path, PathBuf};

/// How deep the listing walks. A research track is two or three levels down;
/// anything much deeper is somebody's dependency tree.
const MAX_DEPTH: usize = 6;
/// How many entries the listing will report. A cap rather than a truncation
/// nobody notices: `truncated` in the response says when it bit.
const MAX_ENTRIES: usize = 500;
/// Refuse to render text past this. The point is that a tab that freezes is
/// worse than a message saying the file is too big.
pub const MAX_TEXT_BYTES: u64 = 2 * 1024 * 1024;

/// Directories never worth walking into. Not a security control (the
/// traversal check is), just the difference between a useful list and ten
/// thousand entries of somebody else's code.
const SKIP_DIRS: [&str; 4] = ["target", "node_modules", ".git", "dist"];

/// What a given extension is served as. An allowlist, deliberately: an
/// unknown type guessed at as `text/html` is a cross-site scripting hole,
/// while an unknown type simply refused is an inconvenience.
pub fn content_type(path: &Path) -> Option<&'static str> {
    let ext = path.extension()?.to_str()?.to_ascii_lowercase();
    match ext.as_str() {
        "md" | "markdown" | "txt" | "text" => Some("text/plain; charset=utf-8"),
        "json" => Some("text/plain; charset=utf-8"),
        "csv" => Some("text/plain; charset=utf-8"),
        "pdf" => Some("application/pdf"),
        _ => None,
    }
}

/// Why a request for a file was refused. Separate from the HTTP layer so the
/// rules can be tested without a server.
#[derive(Debug, Eq, PartialEq)]
pub enum Refusal {
    /// The path resolved to somewhere outside the workspace.
    Outside,
    /// Nothing is there, or it is not a regular file.
    Missing,
    /// A real file, of a type this endpoint will not serve.
    UnsupportedType,
}

/// Resolve a caller-supplied relative path against the workspace root.
///
/// The check that matters happens after canonicalization, not before. Looking
/// for `..` in the string is not enough: a symlink inside the workspace can
/// point anywhere, and it contains no `..` at all. Canonicalizing both sides
/// and then asking whether one is under the other catches both, and catches
/// whatever the next trick is too.
pub fn resolve(root: &Path, requested: &str) -> Result<PathBuf, Refusal> {
    let root = root.canonicalize().map_err(|_| Refusal::Missing)?;

    // An absolute path is not how this endpoint is addressed. Joining one
    // would silently discard the root, which is the classic way this check
    // gets bypassed: `Path::join` with an absolute argument returns the
    // argument.
    let requested = Path::new(requested);
    if requested.is_absolute() {
        return Err(Refusal::Outside);
    }

    let full = root.join(requested);
    let full = full.canonicalize().map_err(|_| Refusal::Missing)?;
    if !full.starts_with(&root) {
        return Err(Refusal::Outside);
    }
    if !full.is_file() {
        return Err(Refusal::Missing);
    }
    if content_type(&full).is_none() {
        return Err(Refusal::UnsupportedType);
    }
    Ok(full)
}

/// One row of the listing.
#[derive(Debug, serde::Serialize)]
pub struct Entry {
    /// Relative to the workspace root, forward slashes, which is exactly what
    /// gets handed back to `resolve`.
    pub path: String,
    pub bytes: u64,
}

#[derive(Debug, serde::Serialize)]
pub struct Listing {
    pub files: Vec<Entry>,
    /// True when `MAX_ENTRIES` cut the list short. A silently truncated list
    /// reads as "that is everything", which it is not.
    pub truncated: bool,
}

/// Walk the workspace for files this endpoint would serve.
///
/// Depth first, skipping the directories in `SKIP_DIRS` and every dot
/// directory except `.zorp`, which is where zorp puts its own output and so
/// is the one place the interesting files actually live.
pub fn list(root: &Path) -> Listing {
    let mut files = Vec::new();
    let mut truncated = false;
    let Ok(root) = root.canonicalize() else {
        return Listing {
            files,
            truncated: false,
        };
    };
    walk(&root, &root, 0, &mut files, &mut truncated);
    files.sort_by(|a, b| a.path.cmp(&b.path));
    Listing { files, truncated }
}

fn walk(root: &Path, dir: &Path, depth: usize, out: &mut Vec<Entry>, truncated: &mut bool) {
    if depth > MAX_DEPTH || *truncated {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        if out.len() >= MAX_ENTRIES {
            *truncated = true;
            return;
        }
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        // `file_type` rather than `is_dir`, so a symlink is not followed
        // during the walk. Serving still resolves and checks it; this just
        // keeps a symlink loop from hanging the listing.
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        if kind.is_symlink() {
            continue;
        }
        if kind.is_dir() {
            if SKIP_DIRS.contains(&name) || (name.starts_with('.') && name != ".zorp") {
                continue;
            }
            walk(root, &path, depth + 1, out, truncated);
            continue;
        }
        if content_type(&path).is_none() {
            continue;
        }
        let bytes = entry.metadata().map(|m| m.len()).unwrap_or(0);
        if let Ok(rel) = path.strip_prefix(root) {
            out.push(Entry {
                path: rel.to_string_lossy().replace('\\', "/"),
                bytes,
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn workspace() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("workspace/.zorp/tracks/t1")).unwrap();
        std::fs::write(
            dir.path().join("workspace/.zorp/tracks/t1/draft.md"),
            "# hi",
        )
        .unwrap();
        std::fs::write(dir.path().join("outside.md"), "secret").unwrap();
        dir
    }

    #[test]
    fn a_relative_path_inside_the_workspace_resolves() {
        let dir = workspace();
        let root = dir.path().join("workspace");
        assert!(resolve(&root, ".zorp/tracks/t1/draft.md").is_ok());
    }

    #[test]
    fn dot_dot_does_not_escape() {
        let dir = workspace();
        let root = dir.path().join("workspace");
        assert_eq!(resolve(&root, "../outside.md"), Err(Refusal::Outside));
    }

    /// `Path::join` returns its argument when that argument is absolute, so
    /// without the explicit check an absolute path would discard the root
    /// and be served from wherever it pointed.
    #[test]
    fn an_absolute_path_is_refused_rather_than_replacing_the_root() {
        let dir = workspace();
        let root = dir.path().join("workspace");
        let absolute = dir.path().join("outside.md");
        assert_eq!(
            resolve(&root, absolute.to_str().unwrap()),
            Err(Refusal::Outside)
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_symlink_out_of_the_workspace_is_refused_even_with_no_dot_dot_in_the_path() {
        let dir = workspace();
        let root = dir.path().join("workspace");
        std::os::unix::fs::symlink(dir.path().join("outside.md"), root.join("escape.md")).unwrap();
        assert_eq!(resolve(&root, "escape.md"), Err(Refusal::Outside));
    }

    #[test]
    fn an_unknown_extension_is_refused_rather_than_guessed_at() {
        let dir = workspace();
        let root = dir.path().join("workspace");
        std::fs::write(root.join("thing.env"), "TOKEN=x").unwrap();
        assert_eq!(resolve(&root, "thing.env"), Err(Refusal::UnsupportedType));
        assert!(content_type(Path::new("x.html")).is_none());
        assert!(content_type(Path::new("x.svg")).is_none());
    }

    #[test]
    fn the_listing_reaches_into_dot_zorp_but_not_into_other_dot_directories() {
        let dir = workspace();
        let root = dir.path().join("workspace");
        std::fs::create_dir_all(root.join(".cache")).unwrap();
        std::fs::write(root.join(".cache/junk.md"), "no").unwrap();

        let paths: Vec<String> = list(&root).files.into_iter().map(|f| f.path).collect();
        assert!(paths.iter().any(|p| p.contains("draft.md")), "{paths:?}");
        assert!(!paths.iter().any(|p| p.starts_with(".cache")), "{paths:?}");
    }
}
