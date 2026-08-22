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
/// Refuse to open a document past this, meaning an office file or a PDF.
/// Higher than the text cap because the file on disk is compressed and the
/// caps that matter for a zip bomb are on what comes out of it, in
/// `crate::documents`. Lower than the binary cap because a document is
/// parsed, and the time that takes grows with the file.
pub const MAX_DOCUMENT_BYTES: u64 = 32 * 1024 * 1024;
/// Refuse to hand a browser bytes past this. An image or an SVG is not parsed
/// here, but it is still read into memory before it goes out.
pub const MAX_BINARY_BYTES: u64 = 64 * 1024 * 1024;

/// Directories never worth walking into. Not a security control (the
/// traversal check is), just the difference between a useful list and ten
/// thousand entries of somebody else's code.
const SKIP_DIRS: [&str; 4] = ["target", "node_modules", ".git", "dist"];

/// How the pane is meant to put a file on screen. The type the bytes are
/// served as follows from this, not the other way round.
///
/// The split that matters is `Sandboxed`. An SVG and an HTML file are both
/// documents a browser will happily execute things inside, so they only ever
/// appear in the pane's sandboxed iframe, never in the page's own DOM.
/// Everything else is either inert bytes (`Image`), text that goes through
/// the markdown renderer, which builds DOM nodes and never assembles markup,
/// or a `Pdf`, which the browser's own viewer draws in a frame of its own.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum Served {
    /// Served as text and rendered by the page.
    Text,
    /// Served as image bytes and shown in an `<img>`.
    Image,
    /// Served as its own type and shown only inside the sandboxed iframe.
    Sandboxed,
    /// Handed to the browser's own PDF viewer, in the pane's PDF frame.
    ///
    /// This is a third framed type and not a third `Sandboxed` one, because
    /// the two go out under different headers and the difference is the whole
    /// point. See `crate::api::read_artifact` for the header, and
    /// `crate::pdf` for the text this falls back to.
    Pdf,
    /// Extracted to markdown on the server, then served and rendered as text.
    Document(Extraction),
}

/// What one response carries. Not the same question as what the file is.
///
/// A PDF on disk is one file and two possible responses: the bytes, for the
/// browser's viewer, or the text read out of them, for when there is no
/// viewer to give them to. `served_as` answers what the file is;
/// `response_type` needs both that and this to answer what the response is.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum Form {
    /// The file as it sits on disk, under its own type.
    Bytes,
    /// The readable text in it, as markdown for the page's own renderer.
    Markdown,
    /// The readable text in it, as plain text for a browser to lay out
    /// itself. This is the fallback that lands inside the PDF frame, where
    /// nothing of ours is rendering and `text/markdown` would be offered as a
    /// download rather than shown.
    PlainText,
}

/// Which reader turns a document into the markdown that goes on the wire.
///
/// Both readers answer the same question, "what does this file say", and
/// neither reproduces how it looked. They are separate because the formats
/// have nothing in common: an office file is a zip of XML and a PDF is a
/// stream of glyph placements.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum Extraction {
    /// One of the office formats. See `crate::documents`.
    Office(crate::documents::Kind),
    /// A PDF, read for the text in it. See `crate::pdf`.
    Pdf,
}

/// How a given extension is handled, or `None` for a type this endpoint will
/// not serve at all. An allowlist, deliberately: an unknown type guessed at as
/// `text/html` is a cross-site scripting hole, while an unknown type simply
/// refused is an inconvenience.
pub fn served_as(path: &Path) -> Option<Served> {
    if let Some(kind) = crate::documents::Kind::for_path(path) {
        return Some(Served::Document(Extraction::Office(kind)));
    }
    let ext = path.extension()?.to_str()?.to_ascii_lowercase();
    match ext.as_str() {
        "md" | "markdown" | "txt" | "text" | "json" | "csv" => Some(Served::Text),
        "png" | "jpg" | "jpeg" | "gif" | "webp" => Some(Served::Image),
        // Sent as itself, for the browser's own viewer to draw. An earlier
        // attempt at this pointed the pane's existing iframe at the file and
        // got a broken-document icon on grey, because that response carries a
        // bare `sandbox` CSP and no viewer starts in an opaque origin with
        // scripting off. The answer is not to stop sending the file, it is to
        // send one `allow-scripts` with it; `crate::api::read_artifact` has
        // the header and what was measured. The text is still read out of it
        // by `crate::pdf` when there is no viewer to hand it to.
        "pdf" => Some(Served::Pdf),
        // These two execute. They are on the list because a run that draws a
        // chart or writes a report page has nowhere else to put it, and they
        // are safe only because the response headers below put them in a
        // sandbox with no script.
        "svg" | "html" => Some(Served::Sandboxed),
        _ => None,
    }
}

/// The `Content-Type` of one response, given the file and what the response
/// was asked to carry.
///
/// Two arguments rather than one, because the file's type and the response's
/// type stopped being the same question when a PDF got two answers. Reading
/// the type off the extension alone is how a response full of extracted text
/// ends up labelled `application/pdf`.
///
/// Never sniffed, and never a fallback: a type `served_as` does not name is
/// refused rather than guessed at. `.svg` gets `image/svg+xml` and `.html`
/// gets `text/html` because that is what they are; the sandbox header on the
/// response, not a mislabelled type, is what keeps them harmless.
pub fn response_type(path: &Path, form: Form) -> Option<&'static str> {
    let served = served_as(path)?;
    match form {
        Form::Markdown => return Some("text/markdown; charset=utf-8"),
        Form::PlainText => return Some("text/plain; charset=utf-8"),
        Form::Bytes => {}
    }
    match served {
        // An office file has no `Bytes` form the pane will ask for: the
        // archive is never what somebody opening the pane wanted.
        Served::Document(_) => Some("text/markdown; charset=utf-8"),
        Served::Text => Some("text/plain; charset=utf-8"),
        Served::Pdf => Some("application/pdf"),
        Served::Image | Served::Sandboxed => {
            let ext = path.extension()?.to_str()?.to_ascii_lowercase();
            match ext.as_str() {
                "png" => Some("image/png"),
                "jpg" | "jpeg" => Some("image/jpeg"),
                "gif" => Some("image/gif"),
                "webp" => Some("image/webp"),
                "svg" => Some("image/svg+xml"),
                "html" => Some("text/html; charset=utf-8"),
                _ => None,
            }
        }
    }
}

/// The reader that turns this file into text, when there is one.
///
/// Separate from `served_as` because they answer different questions now. A
/// PDF is shown as a PDF and read for its text; an office file is only ever
/// read for its text; an image is neither.
pub fn extraction(path: &Path) -> Option<Extraction> {
    match served_as(path)? {
        Served::Document(kind) => Some(kind),
        Served::Pdf => Some(Extraction::Pdf),
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
    if served_as(&full).is_none() {
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
    /// Last modified, in milliseconds since the epoch, or 0 when the
    /// filesystem would not say.
    ///
    /// This is what lets the pane tell "a run wrote this" from "this was
    /// already here": it takes a listing before a turn and compares it with
    /// the listing after. Size alone is not enough, since a rewrite that
    /// happens to land on the same length would look like nothing happened.
    pub modified_ms: u64,
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
        if served_as(&path).is_none() {
            continue;
        }
        let metadata = entry.metadata().ok();
        let bytes = metadata.as_ref().map(|m| m.len()).unwrap_or(0);
        let modified_ms = metadata
            .as_ref()
            .and_then(|m| m.modified().ok())
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_millis().min(u128::from(u64::MAX)) as u64)
            .unwrap_or(0);
        if let Ok(rel) = path.strip_prefix(root) {
            out.push(Entry {
                path: rel.to_string_lossy().replace('\\', "/"),
                bytes,
                modified_ms,
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
        assert!(response_type(Path::new("x.exe"), Form::Bytes).is_none());
        assert!(response_type(Path::new("x.xhtml"), Form::Bytes).is_none());
        assert!(response_type(Path::new("noextension"), Form::Bytes).is_none());
        // Asking for the text of a type this endpoint will not serve does not
        // conjure one up. The file has to be servable first.
        assert!(response_type(Path::new("x.exe"), Form::Markdown).is_none());
    }

    /// The one type that must never be guessed at. `.svg` is an XML document
    /// that can carry script, so serving it as anything but its own type, or
    /// as text the page might inline, is the whole hole.
    #[test]
    fn svg_and_html_are_served_as_themselves_and_only_from_the_iframe() {
        assert_eq!(
            response_type(Path::new("d.svg"), Form::Bytes),
            Some("image/svg+xml")
        );
        assert_eq!(
            response_type(Path::new("r.html"), Form::Bytes),
            Some("text/html; charset=utf-8")
        );
        // These two are what the pane keys on when it decides between the
        // iframe and the page's own DOM. A file that executes must never be
        // anything but Sandboxed.
        assert_eq!(served_as(Path::new("d.svg")), Some(Served::Sandboxed));
        assert_eq!(served_as(Path::new("r.html")), Some(Served::Sandboxed));
        assert_eq!(served_as(Path::new("n.md")), Some(Served::Text));
        assert_eq!(served_as(Path::new("p.png")), Some(Served::Image));
    }

    /// A PDF goes to the browser as a PDF, so the viewer has something to
    /// open. The text read out of it is still there, under `Form::Markdown`,
    /// and that is what the pane asks for when the browser has no viewer.
    #[test]
    fn a_pdf_is_served_as_a_pdf_and_can_still_be_read_for_its_text() {
        assert_eq!(served_as(Path::new("paper.pdf")), Some(Served::Pdf));
        assert_eq!(served_as(Path::new("paper.PDF")), Some(Served::Pdf));
        assert_eq!(
            response_type(Path::new("paper.pdf"), Form::Bytes),
            Some("application/pdf")
        );
        assert_eq!(
            response_type(Path::new("paper.pdf"), Form::Markdown),
            Some("text/markdown; charset=utf-8")
        );
        assert_eq!(
            response_type(Path::new("paper.pdf"), Form::PlainText),
            Some("text/plain; charset=utf-8")
        );
        assert_eq!(extraction(Path::new("paper.pdf")), Some(Extraction::Pdf));
    }

    /// The two types that are `Sandboxed` are the two that execute, and
    /// nothing else may join them by accident.
    ///
    /// A PDF is framed too now, and is deliberately not on this list. The
    /// list is not "what goes in a frame", it is "what goes out under a bare
    /// `Content-Security-Policy: sandbox`", which is a document with no
    /// scripting at all. A PDF cannot be served that way, because the
    /// browser's viewer is itself script and does not start in an opaque
    /// origin with scripting off: that was measured, and it is what the old
    /// broken-document icon was. So a PDF gets `sandbox allow-scripts`
    /// instead, one token wider, and the two lists have to stay apart for the
    /// difference to mean anything. `crate::api::read_artifact` sends both.
    #[test]
    fn only_the_formats_that_execute_are_sandboxed() {
        let all = [
            "a.md", "a.txt", "a.json", "a.csv", "a.png", "a.jpg", "a.gif", "a.webp", "a.pdf",
            "a.docx", "a.odt", "a.xlsx", "a.pptx", "a.svg", "a.html",
        ];
        let sandboxed: Vec<&str> = all
            .into_iter()
            .filter(|name| served_as(Path::new(name)) == Some(Served::Sandboxed))
            .collect();
        assert_eq!(sandboxed, ["a.svg", "a.html"]);

        // And the whole set that reaches a frame, so adding a fourth is a
        // decision somebody has to make here rather than notice later.
        let framed: Vec<&str> = all
            .into_iter()
            .filter(|name| {
                matches!(
                    served_as(Path::new(name)),
                    Some(Served::Sandboxed) | Some(Served::Pdf)
                )
            })
            .collect();
        assert_eq!(framed, ["a.pdf", "a.svg", "a.html"]);
    }

    #[test]
    fn images_are_served_with_their_own_type() {
        for (name, mime) in [
            ("a.png", "image/png"),
            ("a.JPG", "image/jpeg"),
            ("a.jpeg", "image/jpeg"),
            ("a.gif", "image/gif"),
            ("a.webp", "image/webp"),
        ] {
            assert_eq!(response_type(Path::new(name), Form::Bytes), Some(mime));
        }
    }

    /// The bug this whole split exists to stop: a response carrying extracted
    /// text, labelled with the type of the file it was extracted from. An
    /// `application/pdf` full of markdown is a broken viewer, and a
    /// `text/markdown` full of PDF bytes is a page of mojibake.
    #[test]
    fn the_response_type_follows_what_the_response_carries_not_the_extension() {
        assert_ne!(
            response_type(Path::new("paper.pdf"), Form::Markdown),
            Some("application/pdf"),
            "extracted text was labelled as the file it came out of"
        );
        assert_ne!(
            response_type(Path::new("paper.pdf"), Form::Bytes),
            Some("text/markdown; charset=utf-8"),
            "the file itself was labelled as the text in it"
        );
        // An image has no text form to ask for, but asking is not an error;
        // it just means the caller gets told what it would be carrying.
        assert_eq!(extraction(Path::new("shot.png")), None);
        assert_eq!(extraction(Path::new("chart.svg")), None);
        assert_eq!(extraction(Path::new("notes.md")), None);
    }

    /// Office files are extracted to markdown before they leave the server,
    /// so what goes on the wire is text, not the archive.
    #[test]
    fn office_documents_are_served_as_the_markdown_they_extract_to() {
        for name in ["a.docx", "a.odt", "a.xlsx", "a.pptx"] {
            assert!(
                matches!(served_as(Path::new(name)), Some(Served::Document(_))),
                "{name} is not extracted"
            );
            assert_eq!(
                response_type(Path::new(name), Form::Bytes),
                Some("text/markdown; charset=utf-8"),
                "{name}"
            );
        }
    }

    /// The pane diffs one listing against another to notice what a run wrote.
    /// Without a timestamp a file rewritten at the same size is invisible.
    #[test]
    fn the_listing_carries_a_modified_time_for_each_file() {
        let dir = workspace();
        let root = dir.path().join("workspace");
        let listing = list(&root);
        let draft = listing
            .files
            .iter()
            .find(|f| f.path.ends_with("draft.md"))
            .expect("draft.md");
        assert!(
            draft.modified_ms > 0,
            "no modified time on {:?}",
            draft.path
        );
    }

    /// Rewriting a file has to move its timestamp forward, because that is
    /// the only signal the pane gets that a run produced something.
    #[test]
    fn rewriting_a_file_moves_its_modified_time_forward() {
        let dir = workspace();
        let root = dir.path().join("workspace");
        let before = list(&root)
            .files
            .into_iter()
            .find(|f| f.path == "notes.md")
            .map(|f| f.modified_ms);
        assert_eq!(before, None, "the fixture should not have notes.md yet");

        std::fs::write(root.join("notes.md"), "one").unwrap();
        let first = list(&root)
            .files
            .into_iter()
            .find(|f| f.path == "notes.md")
            .expect("notes.md")
            .modified_ms;

        // Filesystem timestamps are not infinitely fine grained, so a rewrite
        // in the same millisecond would compare equal and prove nothing.
        std::thread::sleep(std::time::Duration::from_millis(15));
        std::fs::write(root.join("notes.md"), "one two three").unwrap();
        let second = list(&root)
            .files
            .into_iter()
            .find(|f| f.path == "notes.md")
            .expect("notes.md")
            .modified_ms;

        assert!(second > first, "{second} did not move past {first}");
    }

    #[test]
    fn the_new_formats_show_up_in_the_listing() {
        let dir = workspace();
        let root = dir.path().join("workspace");
        for name in ["chart.svg", "report.html", "shot.png", "memo.docx"] {
            std::fs::write(root.join(name), "x").unwrap();
        }
        let paths: Vec<String> = list(&root).files.into_iter().map(|f| f.path).collect();
        for name in ["chart.svg", "report.html", "shot.png", "memo.docx"] {
            assert!(paths.iter().any(|p| p == name), "{name} missing: {paths:?}");
        }
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
