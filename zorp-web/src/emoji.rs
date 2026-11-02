//! Custom emoji a person adds: image files in `<config>/emoji/`.
//!
//! A custom emoji is a file named `<name>.{png,gif,webp,svg}`, and the name is
//! the key: `:name:` in a message means "the file the server scanned under
//! that name", never a path the message supplied. The name is one component of
//! the shortcode charset, checked before any file is opened, so `../` and a
//! slash cannot appear in it, and a symlink that resolves outside the
//! directory is skipped the way `zorp-skill` skips one.
//!
//! The routes are for a person in the settings pane. No model tool adds,
//! removes or lists custom emoji; `agent.rs` has the test. See issue #243.

use axum::body::Bytes;
use axum::extract::{Path, Query};
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::response::IntoResponse;
use axum::Json;
use serde::Deserialize;
use serde_json::json;
use std::path::PathBuf;

/// An emoji is small. 256 KiB is generous for one and keeps a careless upload
/// from filling the config directory.
pub const MAX_BYTES: usize = 256 * 1024;

/// The types this serves, as `(extension, content type)`, in the order a
/// lookup prefers when a name somehow has more than one file.
const KINDS: &[(&str, &str)] = &[
    ("png", "image/png"),
    ("gif", "image/gif"),
    ("webp", "image/webp"),
    ("svg", "image/svg+xml"),
];

fn dir() -> PathBuf {
    zorp_agent::config::emoji_dir()
}

/// A custom emoji name is one token of the shortcode charset, at most 64
/// characters. The charset excludes `.`, `/` and `\`, so a valid name is
/// already a single path component and cannot traverse; the explicit check is
/// the gate every path below goes through before it touches the filesystem.
pub fn is_valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name.bytes().all(|b| {
            b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'_' | b'+' | b'-')
        })
}

/// The `(extension, content type)` the bytes actually are, by magic number,
/// or `None` for anything this does not serve. Sniffed rather than trusted,
/// because a declared type is the client's word and this writes a file the
/// page will later load.
fn sniff(bytes: &[u8]) -> Option<(&'static str, &'static str)> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        return Some(("png", "image/png"));
    }
    if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        return Some(("gif", "image/gif"));
    }
    if bytes.len() >= 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        return Some(("webp", "image/webp"));
    }
    if looks_like_svg(bytes) {
        return Some(("svg", "image/svg+xml"));
    }
    None
}

fn looks_like_svg(bytes: &[u8]) -> bool {
    let head = &bytes[..bytes.len().min(512)];
    let text = String::from_utf8_lossy(head);
    let trimmed = text.trim_start();
    trimmed.starts_with("<?xml") || trimmed.starts_with("<svg") || text.contains("<svg")
}

/// True when `path` is a symlink that resolves outside `base`, or one whose
/// target cannot be resolved. Such an entry is skipped so a link planted in
/// the directory cannot make the server read or serve a file elsewhere.
fn is_symlink_escape(path: &std::path::Path, base: &std::path::Path) -> bool {
    match std::fs::symlink_metadata(path) {
        Ok(meta) if meta.file_type().is_symlink() => {
            match (std::fs::canonicalize(path), std::fs::canonicalize(base)) {
                (Ok(real), Ok(root)) => !real.starts_with(&root),
                _ => true,
            }
        }
        _ => false,
    }
}

fn plain(status: StatusCode, message: &str) -> axum::response::Response {
    let mut headers = HeaderMap::new();
    headers.insert(
        "content-type",
        HeaderValue::from_static("text/plain; charset=utf-8"),
    );
    headers.insert(
        "x-content-type-options",
        HeaderValue::from_static("nosniff"),
    );
    (status, headers, message.to_string()).into_response()
}

/// The name -> kind-index map of what is installed, lowest `KINDS` index
/// winning when a name has more than one file, symlink-escapes skipped.
fn scan(base: &std::path::Path) -> std::collections::BTreeMap<String, usize> {
    let mut found: std::collections::BTreeMap<String, usize> = std::collections::BTreeMap::new();
    let Ok(read) = std::fs::read_dir(base) else {
        return found;
    };
    for entry in read.flatten() {
        let path = entry.path();
        if is_symlink_escape(&path, base) {
            continue;
        }
        let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
            continue;
        };
        let Some(ext) = path.extension().and_then(|s| s.to_str()) else {
            continue;
        };
        if !is_valid_name(stem) {
            continue;
        }
        let ext = ext.to_ascii_lowercase();
        let Some(idx) = KINDS.iter().position(|(e, _)| *e == ext) else {
            continue;
        };
        found
            .entry(stem.to_string())
            .and_modify(|cur| {
                if idx < *cur {
                    *cur = idx;
                }
            })
            .or_insert(idx);
    }
    found
}

#[derive(Deserialize)]
pub struct AddQuery {
    name: String,
}

/// A served emoji file: its content type, whether it is the sandboxed SVG,
/// and its bytes. The core of `serve`, taking the directory so it is testable
/// without the config environment.
#[derive(Debug)]
pub struct Served {
    pub content_type: &'static str,
    pub is_svg: bool,
    pub body: Vec<u8>,
}

type EmojiErr = (StatusCode, String);

/// The installed custom emoji under `base`, as `(name, content type)`.
pub fn list_in(base: &std::path::Path) -> Vec<(String, &'static str)> {
    scan(base)
        .into_iter()
        .map(|(name, idx)| (name, KINDS[idx].1))
        .collect()
}

/// Find and read the file for `name` under `base`, with the size cap and the
/// name check, or a status and message to refuse with.
pub fn serve_in(base: &std::path::Path, name: &str) -> Result<Served, EmojiErr> {
    if !is_valid_name(name) {
        return Err((StatusCode::BAD_REQUEST, "not a valid emoji name".into()));
    }
    for (ext, content_type) in KINDS {
        let path = base.join(format!("{name}.{ext}"));
        if is_symlink_escape(&path, base) {
            continue;
        }
        let Ok(meta) = std::fs::metadata(&path) else {
            continue;
        };
        if !meta.is_file() {
            continue;
        }
        if meta.len() > MAX_BYTES as u64 {
            return Err((
                StatusCode::PAYLOAD_TOO_LARGE,
                "that emoji file is over the size cap".into(),
            ));
        }
        let Ok(body) = std::fs::read(&path) else {
            continue;
        };
        return Ok(Served {
            content_type,
            is_svg: *ext == "svg",
            body,
        });
    }
    Err((StatusCode::NOT_FOUND, "no such emoji".into()))
}

/// Validate, sniff and write `bytes` as the file for `name` under `base`,
/// replacing any existing file for the name. Returns the name and its type.
pub fn add_in(
    base: &std::path::Path,
    name: &str,
    bytes: &[u8],
) -> Result<(String, &'static str), EmojiErr> {
    if !is_valid_name(name) {
        return Err((
            StatusCode::BAD_REQUEST,
            "an emoji name is 1 to 64 characters of a-z, 0-9, _, + or -".into(),
        ));
    }
    if bytes.len() > MAX_BYTES {
        return Err((
            StatusCode::PAYLOAD_TOO_LARGE,
            "that file is over the 256 KiB cap".into(),
        ));
    }
    let Some((ext, content_type)) = sniff(bytes) else {
        return Err((
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            "that is not a PNG, GIF, WebP or SVG".into(),
        ));
    };
    std::fs::create_dir_all(base).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("could not create the emoji directory: {e}"),
        )
    })?;
    // Replace any existing file for this name, whatever its type, so one name
    // never maps to two files.
    remove_files(base, name);
    std::fs::write(base.join(format!("{name}.{ext}")), bytes).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("could not write the emoji: {e}"),
        )
    })?;
    Ok((name.to_string(), content_type))
}

/// Remove the file(s) for `name` under `base`. Err when the name is bad or
/// nothing was there.
pub fn delete_in(base: &std::path::Path, name: &str) -> Result<(), EmojiErr> {
    if !is_valid_name(name) {
        return Err((StatusCode::BAD_REQUEST, "not a valid emoji name".into()));
    }
    if remove_files(base, name) {
        Ok(())
    } else {
        Err((StatusCode::NOT_FOUND, "no such emoji".into()))
    }
}

/// `GET /api/emoji`: the installed custom emoji, name and content type.
pub async fn list() -> impl IntoResponse {
    let items: Vec<_> = list_in(&dir())
        .into_iter()
        .map(|(name, content_type)| json!({ "name": name, "type": content_type }))
        .collect();
    Json(json!({ "emoji": items }))
}

/// `GET /api/emoji/:name`: serve the file with its own content type, a size
/// cap, `nosniff`, and for an SVG the bare `sandbox` CSP, because an SVG can
/// carry script.
pub async fn serve(Path(name): Path<String>) -> axum::response::Response {
    match serve_in(&dir(), &name) {
        Ok(served) => {
            let mut headers = HeaderMap::new();
            headers.insert(
                "content-type",
                HeaderValue::from_static(served.content_type),
            );
            headers.insert(
                "x-content-type-options",
                HeaderValue::from_static("nosniff"),
            );
            headers.insert("content-disposition", HeaderValue::from_static("inline"));
            if served.is_svg {
                headers.insert(
                    "content-security-policy",
                    HeaderValue::from_static("sandbox"),
                );
            }
            (headers, served.body).into_response()
        }
        Err((status, message)) => plain(status, &message),
    }
}

/// `POST /api/emoji?name=<name>`: the raw file as the body. The type is
/// sniffed, not trusted; a bad name, an oversize file, or a type this does not
/// serve is refused. Any existing file for the name is replaced.
pub async fn add(Query(query): Query<AddQuery>, body: Bytes) -> axum::response::Response {
    match add_in(&dir(), &query.name, &body) {
        Ok((name, content_type)) => (
            StatusCode::OK,
            Json(json!({ "name": name, "type": content_type })),
        )
            .into_response(),
        Err((status, message)) => plain(status, &message),
    }
}

/// `DELETE /api/emoji/:name`: remove the file for the name.
pub async fn delete(Path(name): Path<String>) -> axum::response::Response {
    match delete_in(&dir(), &name) {
        Ok(()) => (StatusCode::OK, Json(json!({ "removed": name }))).into_response(),
        Err((status, message)) => plain(status, &message),
    }
}

/// Remove every `<name>.<ext>` this serves. Returns whether anything went.
fn remove_files(base: &std::path::Path, name: &str) -> bool {
    let mut removed = false;
    for (ext, _) in KINDS {
        let path = base.join(format!("{name}.{ext}"));
        if std::fs::remove_file(&path).is_ok() {
            removed = true;
        }
    }
    removed
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_name_is_a_key_not_a_path() {
        assert!(is_valid_name("partyparrot"));
        assert!(is_valid_name("party_parrot-2"));
        assert!(is_valid_name("+1"));
        assert!(!is_valid_name(""));
        assert!(!is_valid_name("../etc/passwd"));
        assert!(!is_valid_name("a/b"));
        assert!(!is_valid_name("a.b"));
        assert!(!is_valid_name("Upper"));
        assert!(!is_valid_name(&"x".repeat(65)));
    }

    const PNG: &[u8] = b"\x89PNG\r\n\x1a\nnot really but enough";

    #[test]
    fn add_list_serve_and_delete_round_trip() {
        let tmp = tempfile::tempdir().unwrap();
        let base = tmp.path();

        let (name, ct) = add_in(base, "partyparrot", PNG).unwrap();
        assert_eq!(name, "partyparrot");
        assert_eq!(ct, "image/png");

        let listed = list_in(base);
        assert_eq!(listed, vec![("partyparrot".to_string(), "image/png")]);

        let served = serve_in(base, "partyparrot").unwrap();
        assert_eq!(served.content_type, "image/png");
        assert!(!served.is_svg);
        assert_eq!(served.body, PNG);

        delete_in(base, "partyparrot").unwrap();
        assert!(list_in(base).is_empty());
        assert_eq!(
            serve_in(base, "partyparrot").unwrap_err().0,
            StatusCode::NOT_FOUND
        );
    }

    #[test]
    fn add_refuses_a_bad_name_an_oversize_file_and_an_unknown_type() {
        let tmp = tempfile::tempdir().unwrap();
        let base = tmp.path();
        assert_eq!(
            add_in(base, "../evil", PNG).unwrap_err().0,
            StatusCode::BAD_REQUEST
        );
        assert_eq!(
            add_in(base, "big", &vec![0x89u8; MAX_BYTES + 1])
                .unwrap_err()
                .0,
            StatusCode::PAYLOAD_TOO_LARGE
        );
        assert_eq!(
            add_in(base, "textfile", b"just text, not an image")
                .unwrap_err()
                .0,
            StatusCode::UNSUPPORTED_MEDIA_TYPE
        );
        assert!(list_in(base).is_empty());
    }

    #[test]
    fn an_svg_is_marked_for_the_sandbox_and_replaces_an_earlier_type() {
        let tmp = tempfile::tempdir().unwrap();
        let base = tmp.path();
        add_in(base, "logo", PNG).unwrap();
        // Re-adding the same name with a different type leaves exactly one file.
        add_in(
            base,
            "logo",
            b"<svg xmlns=\"http://www.w3.org/2000/svg\"></svg>",
        )
        .unwrap();
        assert_eq!(list_in(base), vec![("logo".to_string(), "image/svg+xml")]);
        let served = serve_in(base, "logo").unwrap();
        assert!(
            served.is_svg,
            "an SVG must be flagged so the route sandboxes it"
        );
    }

    #[test]
    fn a_symlink_escaping_the_directory_is_skipped() {
        let tmp = tempfile::tempdir().unwrap();
        let base = tmp.path().join("emoji");
        std::fs::create_dir_all(&base).unwrap();
        // A real PNG outside the directory, and a symlink to it inside.
        let outside = tmp.path().join("secret.png");
        std::fs::write(&outside, PNG).unwrap();
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(&outside, base.join("sneaky.png")).unwrap();
            assert!(
                list_in(&base).is_empty(),
                "a symlink out of the directory must not be listed"
            );
            assert_eq!(
                serve_in(&base, "sneaky").unwrap_err().0,
                StatusCode::NOT_FOUND
            );
        }
    }

    #[test]
    fn sniff_knows_the_four_types_and_nothing_else() {
        assert_eq!(sniff(b"\x89PNG\r\n\x1a\nxx").map(|k| k.0), Some("png"));
        assert_eq!(sniff(b"GIF89a....").map(|k| k.0), Some("gif"));
        let mut webp = Vec::from(*b"RIFF");
        webp.extend_from_slice(&[0, 0, 0, 0]);
        webp.extend_from_slice(b"WEBPxx");
        assert_eq!(sniff(&webp).map(|k| k.0), Some("webp"));
        assert_eq!(sniff(b"<svg xmlns=\"...\">").map(|k| k.0), Some("svg"));
        assert_eq!(
            sniff(b"<?xml version=\"1.0\"?><svg>").map(|k| k.0),
            Some("svg")
        );
        assert_eq!(sniff(b"not an image"), None);
        assert_eq!(sniff(b"%PDF-1.7"), None);
    }
}
