//! `GET /api/artifacts` and `GET /api/artifacts/raw`.
//!
//! These endpoints hand file contents to a browser, so the tests that matter
//! most are the ones that try to read something outside the workspace. See
//! `docs/superpowers/specs/2026-08-17-artifact-pane-design.md`.

use std::net::SocketAddr;
use std::path::Path;
use zorp_web::state::AppState;

/// A workspace with a couple of artifacts in it, plus a secret one directory
/// up that nothing served from the workspace should ever be able to reach.
struct Workspace {
    dir: tempfile::TempDir,
}

impl Workspace {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("workspace");
        std::fs::create_dir_all(root.join(".zorp/tracks/t1")).unwrap();
        std::fs::write(
            root.join(".zorp/tracks/t1/draft.md"),
            "# Draft\n\nLatency improved.\n",
        )
        .unwrap();
        std::fs::write(root.join("notes.md"), "hello\n").unwrap();
        std::fs::write(
            root.join("paper.pdf"),
            pdf("BT /F1 12 Tf 72 720 Td (Findings) Tj ET"),
        )
        .unwrap();
        std::fs::write(root.join("secret.env"), "TOKEN=hunter2\n").unwrap();
        // The thing traversal would be after: outside the workspace root.
        std::fs::write(dir.path().join("outside.md"), "you should not see this\n").unwrap();
        Workspace { dir }
    }

    fn root(&self) -> std::path::PathBuf {
        self.dir.path().join("workspace")
    }
}

async fn spawn(root: &Path) -> SocketAddr {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let state = AppState::with_token(None).with_workspace(root.to_path_buf());
    tokio::spawn(async move {
        axum::serve(listener, zorp_web::api::router_with_state(state))
            .await
            .unwrap();
    });
    addr
}

fn get(url: &str) -> (u16, String) {
    match ureq::AgentBuilder::new()
        .redirects(0)
        .build()
        .get(url)
        .call()
    {
        Ok(r) => (r.status(), r.into_string().unwrap_or_default()),
        Err(ureq::Error::Status(code, r)) => (code, r.into_string().unwrap_or_default()),
        Err(e) => panic!("{e}"),
    }
}

async fn get_async(url: String) -> (u16, String) {
    tokio::task::spawn_blocking(move || get(&url))
        .await
        .unwrap()
}

/// The three response headers that decide whether a served file can act.
struct ServedHeaders {
    content_type: String,
    nosniff: String,
    csp: String,
}

async fn headers_of(url: String) -> ServedHeaders {
    tokio::task::spawn_blocking(move || {
        // A refusal has headers worth checking too, and a `.pdf` that is not
        // one comes back as 422. Reading them only on success would mean the
        // cases most worth pinning are the ones this cannot look at.
        let r = match ureq::get(&url).call() {
            Ok(r) => r,
            Err(ureq::Error::Status(_, r)) => r,
            Err(e) => panic!("{e}"),
        };
        ServedHeaders {
            content_type: r.header("content-type").unwrap_or_default().to_string(),
            nosniff: r
                .header("x-content-type-options")
                .unwrap_or_default()
                .to_string(),
            csp: r
                .header("content-security-policy")
                .unwrap_or_default()
                .to_string(),
        }
    })
    .await
    .unwrap()
}

/// The whole reason this endpoint needs care. A path that climbs out of the
/// workspace must be refused, and the refusal must not leak the contents on
/// the way out.
#[tokio::test]
async fn a_path_that_climbs_out_of_the_workspace_is_refused() {
    let ws = Workspace::new();
    let addr = spawn(&ws.root()).await;

    for attempt in [
        "../outside.md",
        "../../etc/passwd",
        ".zorp/../../outside.md",
        "/etc/passwd",
    ] {
        let encoded = attempt.replace("/", "%2F").replace("..", "%2E%2E");
        let (status, body) =
            get_async(format!("http://{addr}/api/artifacts/raw?path={encoded}")).await;
        assert!(
            status == 403 || status == 404,
            "{attempt} was not refused, got {status}: {body}"
        );
        assert!(
            !body.contains("you should not see this") && !body.contains("root:"),
            "{attempt} leaked contents: {body}"
        );
    }
}

/// An absolute path that happens to point inside the workspace is still not
/// how this endpoint is addressed. Everything is relative to the root.
#[tokio::test]
async fn an_ordinary_file_inside_the_workspace_is_served() {
    let ws = Workspace::new();
    let addr = spawn(&ws.root()).await;

    let (status, body) = get_async(format!(
        "http://{addr}/api/artifacts/raw?path=.zorp%2Ftracks%2Ft1%2Fdraft.md"
    ))
    .await;
    assert_eq!(status, 200, "body: {body}");
    assert!(body.contains("Latency improved"), "body: {body}");
}

/// An extension allowlist, not a denylist. An unknown type served as
/// `text/html` would be a cross-site scripting hole, so unknown types are
/// refused rather than guessed at.
#[tokio::test]
async fn a_file_type_that_is_not_on_the_allowlist_is_refused() {
    let ws = Workspace::new();
    let addr = spawn(&ws.root()).await;

    let (status, body) =
        get_async(format!("http://{addr}/api/artifacts/raw?path=secret.env")).await;
    assert_eq!(status, 415, "body: {body}");
    assert!(
        !body.contains("hunter2"),
        "the refusal served the file anyway: {body}"
    );
}

/// A served file must not be able to act as a document in this origin. Both
/// headers matter: `nosniff` stops the browser second-guessing the declared
/// type, and the sandbox CSP stops a served document reaching the rest of the
/// page.
///
/// Every type, not one of them. It is a per-type decision now, which is
/// exactly why the whole list is walked: the moment a type gets it wrong it
/// is a hole, and this test is what would notice.
#[tokio::test]
async fn served_files_carry_the_headers_that_stop_them_becoming_active() {
    let ws = Workspace::new();
    std::fs::write(ws.root().join("chart.svg"), "<svg/>").unwrap();
    std::fs::write(ws.root().join("report.html"), "<html>hi</html>").unwrap();
    std::fs::write(ws.root().join("shot.png"), b"\x89PNG\r\n\x1a\n").unwrap();
    let addr = spawn(&ws.root()).await;

    for name in ["notes.md", "chart.svg", "report.html", "shot.png"] {
        let headers = headers_of(format!("http://{addr}/api/artifacts/raw?path={name}")).await;
        assert_eq!(headers.nosniff, "nosniff", "{name}");
        assert_eq!(
            headers.csp, "sandbox",
            "{name} was served with a widened sandbox"
        );
        assert!(
            !headers.csp.contains("allow-"),
            "{name} csp was {:?}",
            headers.csp
        );
    }
}

/// The one exception, and the exact shape of it.
///
/// A PDF goes out under `sandbox allow-scripts`, because the browser's own
/// viewer is a scripted document and does not start in an opaque origin with
/// scripting off. That was the previous attempt's broken-document icon.
/// `allow-scripts` is the only token added: `allow-same-origin` is what would
/// hand the framed document this page's origin, and it is not there, so
/// `parent.document` and `localStorage` still throw from inside the frame.
///
/// Asked for its text instead, the same file is text and goes back to the
/// bare policy. A response that carries markdown has no viewer to start.
#[tokio::test]
async fn a_pdf_widens_the_sandbox_by_exactly_one_token_and_only_as_a_pdf() {
    let ws = Workspace::new();
    let addr = spawn(&ws.root()).await;

    let framed = headers_of(format!("http://{addr}/api/artifacts/raw?path=paper.pdf")).await;
    assert_eq!(framed.csp, "sandbox allow-scripts");
    assert!(
        !framed.csp.contains("allow-same-origin"),
        "csp was {:?}, which gives the frame this page's origin",
        framed.csp
    );
    assert_eq!(framed.nosniff, "nosniff");

    let read = headers_of(format!(
        "http://{addr}/api/artifacts/raw?path=paper.pdf&as=text"
    ))
    .await;
    assert_eq!(read.csp, "sandbox");
}

/// The listing is what the pane's file list is built from. It finds the
/// artifacts and leaves out the noise.
#[tokio::test]
async fn the_listing_finds_artifacts_and_skips_the_noise() {
    let ws = Workspace::new();
    std::fs::create_dir_all(ws.root().join("target/debug")).unwrap();
    std::fs::write(ws.root().join("target/debug/junk.md"), "build output").unwrap();
    std::fs::create_dir_all(ws.root().join("node_modules/pkg")).unwrap();
    std::fs::write(ws.root().join("node_modules/pkg/readme.md"), "dep").unwrap();
    let addr = spawn(&ws.root()).await;

    let (status, body) = get_async(format!("http://{addr}/api/artifacts")).await;
    assert_eq!(status, 200, "body: {body}");
    let json: serde_json::Value = serde_json::from_str(&body).expect("valid JSON");
    let paths: Vec<String> = json["files"]
        .as_array()
        .expect("files array")
        .iter()
        .map(|f| f["path"].as_str().unwrap_or_default().to_string())
        .collect();

    assert!(
        paths.iter().any(|p| p.contains("draft.md")),
        "draft.md missing from {paths:?}"
    );
    assert!(
        paths.iter().any(|p| p == "notes.md"),
        "notes.md missing from {paths:?}"
    );
    assert!(
        paths.iter().any(|p| p == "paper.pdf"),
        "paper.pdf missing from {paths:?}"
    );
    assert!(
        !paths.iter().any(|p| p.starts_with("target/")),
        "build output was listed: {paths:?}"
    );
    assert!(
        !paths.iter().any(|p| p.starts_with("node_modules/")),
        "dependencies were listed: {paths:?}"
    );
    assert!(
        !paths.iter().any(|p| p.ends_with(".env")),
        "a non-artifact file was listed: {paths:?}"
    );
}

/// A file that vanished between listing and opening is a 404 the pane can
/// explain, not an empty body that looks like an empty file.
#[tokio::test]
async fn a_file_that_is_not_there_is_a_404_rather_than_an_empty_body() {
    let ws = Workspace::new();
    let addr = spawn(&ws.root()).await;

    let (status, body) = get_async(format!("http://{addr}/api/artifacts/raw?path=gone.md")).await;
    assert_eq!(status, 404, "body: {body}");
    assert!(!body.is_empty(), "a 404 with no explanation");
}

/* ------------------------------------------------------------------ */
/* the formats that execute                                            */
/* ------------------------------------------------------------------ */

/// An SVG is an XML document, and an XML document can carry a `<script>`.
/// The pane shows it, so the question is not whether the script is there but
/// whether it can run and what it could reach if it did.
///
/// Two headers answer that, and this test is about both. `nosniff` stops the
/// browser deciding for itself what the response is. A bare `sandbox` CSP
/// directive, with no `allow-` token after it, puts the document in a unique
/// origin with scripting switched off, so the script neither executes nor has
/// a same-origin handle on the page that framed it. Adding a single
/// `allow-scripts` or `allow-same-origin` to that header would undo it, which
/// is why this asserts on the exact value rather than on a substring.
#[tokio::test]
async fn a_served_svg_is_sandboxed_so_script_inside_it_cannot_run_or_reach_the_page() {
    let ws = Workspace::new();
    std::fs::write(
        ws.root().join("chart.svg"),
        r#"<svg xmlns="http://www.w3.org/2000/svg"><script>parent.document.title="pwned"</script></svg>"#,
    )
    .unwrap();
    let addr = spawn(&ws.root()).await;

    let headers = headers_of(format!("http://{addr}/api/artifacts/raw?path=chart.svg")).await;
    assert_eq!(headers.content_type, "image/svg+xml");
    assert_eq!(headers.nosniff, "nosniff");
    assert_eq!(
        headers.csp, "sandbox",
        "the sandbox was widened; any allow- token here lets the script run"
    );
    assert!(!headers.csp.contains("allow-"), "csp was {:?}", headers.csp);
}

/// The same argument for `.html`, which is the case where it is most obvious
/// and therefore easiest to get casually wrong.
#[tokio::test]
async fn a_served_html_file_is_sandboxed_so_its_script_cannot_run_or_reach_the_page() {
    let ws = Workspace::new();
    std::fs::write(
        ws.root().join("report.html"),
        "<html><body><script>top.location='http://evil.example'</script>hi</body></html>",
    )
    .unwrap();
    let addr = spawn(&ws.root()).await;

    let headers = headers_of(format!("http://{addr}/api/artifacts/raw?path=report.html")).await;
    assert_eq!(headers.content_type, "text/html; charset=utf-8");
    assert_eq!(headers.nosniff, "nosniff");
    assert_eq!(
        headers.csp, "sandbox",
        "the sandbox was widened; any allow- token here lets the script run"
    );
    assert!(!headers.csp.contains("allow-"), "csp was {:?}", headers.csp);
}

/// An SVG must not be served as anything the page would be tempted to inline.
/// Serving it as `text/plain` would be the tempting mistake, because the pane
/// already has a text path and text goes into the page's own DOM.
#[tokio::test]
async fn an_svg_is_never_served_as_text() {
    let ws = Workspace::new();
    std::fs::write(ws.root().join("chart.svg"), "<svg/>").unwrap();
    let addr = spawn(&ws.root()).await;

    let headers = headers_of(format!("http://{addr}/api/artifacts/raw?path=chart.svg")).await;
    assert!(
        !headers.content_type.starts_with("text/plain"),
        "an svg served as text would be inlined by the pane: {:?}",
        headers.content_type
    );
}

#[tokio::test]
async fn an_image_is_served_with_its_own_type() {
    let ws = Workspace::new();
    std::fs::write(ws.root().join("shot.png"), b"\x89PNG\r\n\x1a\n").unwrap();
    let addr = spawn(&ws.root()).await;

    let headers = headers_of(format!("http://{addr}/api/artifacts/raw?path=shot.png")).await;
    assert_eq!(headers.content_type, "image/png");
    assert_eq!(headers.nosniff, "nosniff");
}

/* ------------------------------------------------------------------ */
/* office formats                                                      */
/* ------------------------------------------------------------------ */

fn docx(document_xml: &str) -> Vec<u8> {
    use std::io::Write;
    let mut buffer = Vec::new();
    {
        let mut writer = zip::ZipWriter::new(std::io::Cursor::new(&mut buffer));
        writer
            .start_file(
                "word/document.xml",
                zip::write::SimpleFileOptions::default(),
            )
            .unwrap();
        writer.write_all(document_xml.as_bytes()).unwrap();
        writer.finish().unwrap();
    }
    buffer
}

/// The archive is never what goes on the wire. Extraction happens on the
/// server and the browser gets markdown, which the pane already renders
/// without ever assembling markup.
#[tokio::test]
async fn a_docx_is_served_as_the_markdown_it_extracts_to() {
    let ws = Workspace::new();
    std::fs::write(
        ws.root().join("memo.docx"),
        docx(
            r#"<w:document xmlns:w="x"><w:body>
                 <w:p><w:pPr><w:pStyle w:val="Heading1"/></w:pPr><w:r><w:t>Memo</w:t></w:r></w:p>
                 <w:p><w:r><w:t>Two sentences.</w:t></w:r></w:p>
               </w:body></w:document>"#,
        ),
    )
    .unwrap();
    let addr = spawn(&ws.root()).await;

    let url = format!("http://{addr}/api/artifacts/raw?path=memo.docx");
    let headers = headers_of(url.clone()).await;
    assert_eq!(headers.content_type, "text/markdown; charset=utf-8");

    let (status, body) = get_async(url).await;
    assert_eq!(status, 200, "body: {body}");
    assert!(body.contains("# Memo"), "body: {body}");
    assert!(body.contains("Two sentences."), "body: {body}");
    assert!(
        !body.contains("PK\u{3}\u{4}"),
        "the archive itself was served: {body:?}"
    );
}

/* ------------------------------------------------------------------ */
/* pdf                                                                 */
/* ------------------------------------------------------------------ */

/// A one page PDF holding the given content stream operators.
///
/// The same fixture as the one in `zorp-web/src/pdf.rs`, written out here
/// because an integration test cannot reach into the crate's own test module.
/// Built in code rather than checked in as bytes, so it can be read.
fn pdf(content: &str) -> Vec<u8> {
    let objects = [
        "<</Type/Catalog/Pages 2 0 R>>".to_string(),
        "<</Type/Pages/Kids[3 0 R]/Count 1>>".to_string(),
        "<</Type/Page/Parent 2 0 R/MediaBox[0 0 612 792]\
         /Resources<</Font<</F1 4 0 R>>>>/Contents 5 0 R>>"
            .to_string(),
        "<</Type/Font/Subtype/Type1/BaseFont/Helvetica>>".to_string(),
        format!(
            "<</Length {}>>\nstream\n{content}\nendstream",
            content.len() + 1
        ),
    ];

    let mut out = Vec::from(&b"%PDF-1.4\n"[..]);
    let mut offsets = Vec::new();
    for (index, body) in objects.iter().enumerate() {
        offsets.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n{body}\nendobj\n", index + 1).as_bytes());
    }

    let xref = out.len();
    out.extend_from_slice(format!("xref\n0 {}\n", objects.len() + 1).as_bytes());
    out.extend_from_slice(b"0000000000 65535 f \n");
    for offset in &offsets {
        out.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(
        format!(
            "trailer\n<</Size {}/Root 1 0 R>>\nstartxref\n{xref}\n%%EOF\n",
            objects.len() + 1
        )
        .as_bytes(),
    );
    out
}

/// The file itself is what goes on the wire, so the browser's viewer has a
/// PDF to open. This is the bug: the pane showed the words out of the file
/// instead of the file, and a PDF that a run produced is meant to look like a
/// PDF.
#[tokio::test]
async fn a_pdf_is_served_as_a_pdf() {
    let ws = Workspace::new();
    std::fs::write(
        ws.root().join("report.pdf"),
        pdf("BT /F1 18 Tf 72 720 Td (Findings) Tj ET\n\
             BT /F1 12 Tf 72 700 Td (Latency fell by 40 percent.) Tj ET"),
    )
    .unwrap();
    let addr = spawn(&ws.root()).await;

    let url = format!("http://{addr}/api/artifacts/raw?path=report.pdf");
    let headers = headers_of(url.clone()).await;
    assert_eq!(headers.content_type, "application/pdf");

    let (status, body) = get_async(url).await;
    assert_eq!(status, 200, "body: {body}");
    assert!(
        body.starts_with("%PDF-"),
        "the bytes were not the file: {:?}",
        &body[..body.len().min(40)]
    );
}

/// The fallback, and the reason `crate::pdf` is still here. A browser with no
/// PDF viewer asks for the words instead, and gets markdown, like a `.docx`.
#[tokio::test]
async fn the_same_pdf_asked_for_as_text_comes_back_as_the_words_in_it() {
    let ws = Workspace::new();
    std::fs::write(
        ws.root().join("report.pdf"),
        pdf("BT /F1 18 Tf 72 720 Td (Findings) Tj ET\n\
             BT /F1 12 Tf 72 700 Td (Latency fell by 40 percent.) Tj ET"),
    )
    .unwrap();
    let addr = spawn(&ws.root()).await;

    let url = format!("http://{addr}/api/artifacts/raw?path=report.pdf&as=text");
    let headers = headers_of(url.clone()).await;
    assert_eq!(headers.content_type, "text/markdown; charset=utf-8");

    let (status, body) = get_async(url).await;
    assert_eq!(status, 200, "body: {body}");
    assert!(body.contains("Findings"), "body: {body}");
    assert!(body.contains("Latency fell by 40 percent."), "body: {body}");
    assert!(
        !body.contains("%PDF"),
        "the file itself was served: {body:?}"
    );
}

/// `as=text` is about documents with a reader behind them. On anything else
/// it changes nothing, rather than turning a `.png` or a `.html` into some
/// other kind of response.
#[tokio::test]
async fn asking_for_the_text_of_a_file_that_has_no_reader_changes_nothing() {
    let ws = Workspace::new();
    std::fs::write(ws.root().join("report.html"), "<html>hi</html>").unwrap();
    let addr = spawn(&ws.root()).await;

    for (name, mime) in [
        ("notes.md", "text/plain; charset=utf-8"),
        ("report.html", "text/html; charset=utf-8"),
    ] {
        let headers = headers_of(format!(
            "http://{addr}/api/artifacts/raw?path={name}&as=text"
        ))
        .await;
        assert_eq!(headers.content_type, mime, "{name}");
        assert_eq!(headers.csp, "sandbox", "{name}");
    }
}

/// A model wrote this file, or downloaded it, so "it is not really a PDF" is
/// an ordinary case, and now it is one the viewer would draw a
/// broken-document icon over. So the header is checked before the bytes go
/// out and a file that fails it is read for its words instead. A message, not
/// a blank pane and not a 500.
#[tokio::test]
async fn a_pdf_that_is_not_a_pdf_is_read_for_its_words_rather_than_framed() {
    let ws = Workspace::new();
    std::fs::write(ws.root().join("broken.pdf"), b"not a pdf at all").unwrap();
    let addr = spawn(&ws.root()).await;

    let url = format!("http://{addr}/api/artifacts/raw?path=broken.pdf");
    let headers = headers_of(url.clone()).await;
    assert_ne!(
        headers.content_type, "application/pdf",
        "a file with no PDF header was handed to the viewer anyway"
    );
    assert_eq!(
        headers.csp, "sandbox",
        "the sandbox was widened for something that is not a PDF"
    );

    let (status, body) = get_async(url).await;
    assert_eq!(status, 422, "body: {body}");
    assert!(body.contains("not a readable PDF"), "body: {body}");
}

/// The same gate, for a file that is not a PDF but does hold readable text.
/// The pane shows the words inside the frame, so they go out as plain text: a
/// browser handed `text/markdown` offers to save it instead of showing it.
#[tokio::test]
async fn a_pdf_header_missing_but_words_present_comes_back_as_plain_text() {
    let ws = Workspace::new();
    // A `.pdf` that is really markdown, which is what a model writes when it
    // means to produce a PDF and does not.
    std::fs::write(
        ws.root().join("draft.pdf"),
        b"# Findings\n\nLatency fell.\n",
    )
    .unwrap();
    let addr = spawn(&ws.root()).await;

    let url = format!("http://{addr}/api/artifacts/raw?path=draft.pdf");
    let headers = headers_of(url.clone()).await;
    assert_ne!(headers.content_type, "application/pdf");
    // 422 with the reader's sentence is the honest answer for a file the
    // reader cannot read, and either way the frame shows words.
    let (status, body) = get_async(url).await;
    assert!(status == 200 || status == 422, "{status}: {body}");
    assert!(!body.is_empty(), "the frame would have been empty");
}

/* ------------------------------------------------------------------ */
/* office formats, continued                                           */
/* ------------------------------------------------------------------ */

/// A model wrote this file, so "it is not really a docx" is an ordinary case.
/// It gets a message, not a blank pane and not a 500.
#[tokio::test]
async fn a_docx_that_is_not_an_archive_is_refused_with_a_reason() {
    let ws = Workspace::new();
    std::fs::write(ws.root().join("broken.docx"), b"not a zip at all").unwrap();
    let addr = spawn(&ws.root()).await;

    let (status, body) =
        get_async(format!("http://{addr}/api/artifacts/raw?path=broken.docx")).await;
    assert_eq!(status, 422, "body: {body}");
    assert!(
        body.contains("not a readable office document"),
        "body: {body}"
    );
}

/* ------------------------------------------------------------------ */
/* the listing                                                         */
/* ------------------------------------------------------------------ */

/// The pane notices what a run wrote by diffing one listing against the next,
/// which needs a timestamp on every row.
#[tokio::test]
async fn the_listing_reports_when_each_file_last_changed() {
    let ws = Workspace::new();
    let addr = spawn(&ws.root()).await;

    let (status, body) = get_async(format!("http://{addr}/api/artifacts")).await;
    assert_eq!(status, 200, "body: {body}");
    let json: serde_json::Value = serde_json::from_str(&body).expect("valid JSON");
    for file in json["files"].as_array().expect("files array") {
        let modified = file["modified_ms"].as_u64();
        assert!(
            modified.is_some_and(|ms| ms > 0),
            "no modified_ms on {file:?}"
        );
    }
}

/// A symlink is the other way out of the workspace, and it is not caught by
/// looking for `..` in the path. The check has to happen after resolving.
#[cfg(unix)]
#[tokio::test]
async fn a_symlink_pointing_out_of_the_workspace_is_refused() {
    let ws = Workspace::new();
    std::os::unix::fs::symlink(
        ws.dir.path().join("outside.md"),
        ws.root().join("escape.md"),
    )
    .unwrap();
    let addr = spawn(&ws.root()).await;

    let (status, body) = get_async(format!("http://{addr}/api/artifacts/raw?path=escape.md")).await;
    assert!(
        status == 403 || status == 404,
        "a symlink out of the workspace was followed, got {status}: {body}"
    );
    assert!(
        !body.contains("you should not see this"),
        "the symlink leaked its target: {body}"
    );
}
