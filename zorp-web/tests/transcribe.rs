//! `POST /api/transcribe`.
//!
//! The endpoint forwards recorded speech to a transcription server the user
//! configured, and does nothing else. What these tests are really pinning
//! down is where the audio goes and what rides along with it: the request
//! must be an ordinary OpenAI-shaped upload, it must carry no credentials,
//! and a server that answers with something other than a transcript must
//! not have that something turned into text the user is invited to send.

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::Mutex as AsyncMutex;
use zorp_web::state::AppState;

/// Transcription settings resolve against real process env vars and a real
/// config file path, so these tests serialise the same way the settings
/// tests do.
static ENV: AsyncMutex<()> = AsyncMutex::const_new(());

struct Isolated {
    _dir: tempfile::TempDir,
}

impl Isolated {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        std::env::set_var("ZORP_WEB_CONFIG", dir.path().join("web.toml"));
        for var in [
            "ZORP_PROVIDER",
            "ZORP_BASE_URL",
            "ZORP_MODEL",
            "ZORP_API_KEY",
            "ZORP_MAX_TOKENS",
            "ZORP_TRANSCRIBE_BASE_URL",
            "ZORP_TRANSCRIBE_MODEL",
        ] {
            std::env::remove_var(var);
        }
        Isolated { _dir: dir }
    }
}

/// A stand-in transcription server. Answers one request with whatever it
/// was told to, and keeps the request so a test can look at what was sent.
struct Upstream {
    addr: SocketAddr,
    request: Arc<Mutex<Vec<u8>>>,
}

impl Upstream {
    fn answering(status: u16, content_type: &str, body: &str) -> Upstream {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let request = Arc::new(Mutex::new(Vec::new()));
        let seen = Arc::clone(&request);
        let response = format!(
            "HTTP/1.1 {status} X\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\n\
             Connection: close\r\n\r\n{body}",
            body.len()
        );
        std::thread::spawn(move || {
            let Ok((mut socket, _)) = listener.accept() else {
                return;
            };
            let mut buffer = Vec::new();
            let mut chunk = [0u8; 8192];
            // Read headers first, then exactly as many body bytes as the
            // request said it would send. Reading to EOF instead would
            // block until the client hangs up, which it will not do until
            // it has an answer.
            let length = loop {
                let read = socket.read(&mut chunk).unwrap_or(0);
                if read == 0 {
                    break 0;
                }
                buffer.extend_from_slice(&chunk[..read]);
                if let Some(at) = find(&buffer, b"\r\n\r\n") {
                    let headers = String::from_utf8_lossy(&buffer[..at]).to_lowercase();
                    let declared = headers
                        .lines()
                        .find_map(|line| line.strip_prefix("content-length:"))
                        .and_then(|v| v.trim().parse::<usize>().ok())
                        .unwrap_or(0);
                    break at + 4 + declared;
                }
            };
            while buffer.len() < length {
                let read = socket.read(&mut chunk).unwrap_or(0);
                if read == 0 {
                    break;
                }
                buffer.extend_from_slice(&chunk[..read]);
            }
            *seen.lock().unwrap() = buffer;
            let _ = socket.write_all(response.as_bytes());
            let _ = socket.flush();
        });
        Upstream { addr, request }
    }

    fn base_url(&self) -> String {
        format!("http://{}", self.addr)
    }

    /// Everything the server received, with the audio bytes made printable.
    fn seen(&self) -> String {
        // Give the thread a moment to record the request before reading it.
        for _ in 0..100 {
            if !self.request.lock().unwrap().is_empty() {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        String::from_utf8_lossy(&self.request.lock().unwrap()).into_owned()
    }
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

async fn spawn() -> SocketAddr {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(
            listener,
            zorp_web::api::router_with_state(AppState::with_token(None)),
        )
        .await
        .unwrap();
    });
    addr
}

fn put(url: &str, body: &str) -> (u16, String) {
    match ureq::put(url)
        .set("content-type", "application/json")
        .send_string(body)
    {
        Ok(r) => (r.status(), r.into_string().unwrap_or_default()),
        Err(ureq::Error::Status(code, r)) => (code, r.into_string().unwrap_or_default()),
        Err(e) => panic!("{e}"),
    }
}

fn post_audio(url: &str, bytes: Vec<u8>) -> (u16, String) {
    match ureq::post(url)
        .set("content-type", "audio/wav")
        .send_bytes(&bytes)
    {
        Ok(r) => (r.status(), r.into_string().unwrap_or_default()),
        Err(ureq::Error::Status(code, r)) => (code, r.into_string().unwrap_or_default()),
        Err(e) => panic!("{e}"),
    }
}

async fn blocking<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> T {
    tokio::task::spawn_blocking(f).await.unwrap()
}

/// A minimal but genuine 16 kHz mono WAV, so the endpoint's own shape check
/// is exercised rather than side-stepped.
fn wav(samples: usize) -> Vec<u8> {
    let data = samples * 2;
    let mut out = Vec::new();
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&((36 + data) as u32).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&16000u32.to_le_bytes());
    out.extend_from_slice(&32000u32.to_le_bytes());
    out.extend_from_slice(&2u16.to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&(data as u32).to_le_bytes());
    out.extend(std::iter::repeat(0u8).take(data));
    out
}

/* ------------------------------------------------------------------ */

/// Not a dead button and not a silent no-op: the server says what is
/// missing. A feature that quietly does nothing on a machine without its
/// dependency is the failure this whole design is arranged around.
#[tokio::test]
async fn with_nothing_configured_the_endpoint_says_what_is_missing() {
    let _env = ENV.lock().await;
    let _iso = Isolated::new();
    let addr = spawn().await;

    let (status, body) =
        blocking(move || post_audio(&format!("http://{addr}/api/transcribe"), wav(8))).await;
    assert_eq!(status, 503, "body: {body}");
    assert!(
        body.contains("transcribe") || body.contains("transcription"),
        "the refusal did not name the missing setting: {body}"
    );
}

#[tokio::test]
async fn an_empty_recording_is_refused_before_anything_is_forwarded() {
    let _env = ENV.lock().await;
    let _iso = Isolated::new();
    let addr = spawn().await;
    let upstream = Upstream::answering(200, "application/json", r#"{"text":"never"}"#);

    let base = upstream.base_url();
    let (put_status, put_body) = blocking(move || {
        put(
            &format!("http://{addr}/api/settings"),
            &format!(r#"{{"transcribe_base_url":"{base}"}}"#),
        )
    })
    .await;
    assert_eq!(put_status, 200, "body: {put_body}");

    let (status, body) =
        blocking(move || post_audio(&format!("http://{addr}/api/transcribe"), Vec::new())).await;
    assert_eq!(status, 400, "body: {body}");
}

#[tokio::test]
async fn a_body_that_is_not_a_wav_is_refused() {
    let _env = ENV.lock().await;
    let _iso = Isolated::new();
    let addr = spawn().await;
    let upstream = Upstream::answering(200, "application/json", r#"{"text":"never"}"#);

    let base = upstream.base_url();
    blocking(move || {
        put(
            &format!("http://{addr}/api/settings"),
            &format!(r#"{{"transcribe_base_url":"{base}"}}"#),
        )
    })
    .await;

    let (status, body) = blocking(move || {
        post_audio(
            &format!("http://{addr}/api/transcribe"),
            b"{\"not\":\"audio\"}".to_vec(),
        )
    })
    .await;
    assert_eq!(status, 400, "body: {body}");
}

#[tokio::test]
async fn a_transcription_comes_back_as_text() {
    let _env = ENV.lock().await;
    let _iso = Isolated::new();
    let addr = spawn().await;
    let upstream = Upstream::answering(
        200,
        "application/json",
        r#"{"text":"run the tests and tell me what broke"}"#,
    );

    let base = upstream.base_url();
    blocking(move || {
        put(
            &format!("http://{addr}/api/settings"),
            &format!(r#"{{"transcribe_base_url":"{base}"}}"#),
        )
    })
    .await;

    let (status, body) =
        blocking(move || post_audio(&format!("http://{addr}/api/transcribe"), wav(16))).await;
    assert_eq!(status, 200, "body: {body}");
    let json: serde_json::Value = serde_json::from_str(&body).expect("valid JSON");
    assert_eq!(json["text"], "run the tests and tell me what broke");
}

/// The wire shape whisper.cpp, speaches and OpenAI all read: a multipart
/// form with the audio under `file`. Getting the part name wrong produces a
/// 400 from the endpoint that reads like the audio was bad.
#[tokio::test]
async fn the_upstream_request_is_an_openai_shaped_multipart_upload() {
    let _env = ENV.lock().await;
    let _iso = Isolated::new();
    let addr = spawn().await;
    let upstream = Upstream::answering(200, "application/json", r#"{"text":"ok"}"#);

    let base = upstream.base_url();
    blocking(move || {
        put(
            &format!("http://{addr}/api/settings"),
            &format!(r#"{{"transcribe_base_url":"{base}","transcribe_model":"whisper-large"}}"#),
        )
    })
    .await;

    blocking(move || post_audio(&format!("http://{addr}/api/transcribe"), wav(16))).await;

    let seen = upstream.seen();
    assert!(
        seen.starts_with("POST /audio/transcriptions "),
        "wrong method or path: {}",
        seen.lines().next().unwrap_or_default()
    );
    let lowered = seen.to_lowercase();
    assert!(
        lowered.contains("content-type: multipart/form-data; boundary="),
        "not a multipart upload: {lowered}"
    );
    assert!(seen.contains(r#"name="file""#), "no file part: {seen}");
    assert!(seen.contains("filename="), "the file part had no filename");
    assert!(seen.contains(r#"name="model""#), "no model part");
    assert!(
        seen.contains("whisper-large"),
        "the configured model was not sent"
    );
    assert!(seen.contains("RIFF"), "the audio itself was not sent");
}

/// The transcription endpoint is a local server, and zorp never
/// authenticates to it. A configured chat API key belongs to a different
/// host, and sending it here would hand one provider's credential to
/// another machine.
#[tokio::test]
async fn the_api_key_is_never_sent_to_the_transcription_endpoint() {
    let _env = ENV.lock().await;
    let _iso = Isolated::new();
    let addr = spawn().await;
    let upstream = Upstream::answering(200, "application/json", r#"{"text":"ok"}"#);

    let base = upstream.base_url();
    blocking(move || {
        put(
            &format!("http://{addr}/api/settings"),
            &format!(
                r#"{{"transcribe_base_url":"{base}","api_key":"sk-must-not-travel-with-audio"}}"#
            ),
        )
    })
    .await;

    blocking(move || post_audio(&format!("http://{addr}/api/transcribe"), wav(16))).await;

    let seen = upstream.seen();
    assert!(
        !seen.contains("sk-must-not-travel-with-audio"),
        "the chat API key was sent to the transcription endpoint: {seen}"
    );
    assert!(
        !seen.to_lowercase().contains("authorization:"),
        "the request was authenticated: {seen}"
    );
}

#[tokio::test]
async fn an_upstream_failure_is_reported_rather_than_swallowed() {
    let _env = ENV.lock().await;
    let _iso = Isolated::new();
    let addr = spawn().await;
    let upstream = Upstream::answering(500, "application/json", r#"{"error":"no model loaded"}"#);

    let base = upstream.base_url();
    blocking(move || {
        put(
            &format!("http://{addr}/api/settings"),
            &format!(r#"{{"transcribe_base_url":"{base}"}}"#),
        )
    })
    .await;

    let (status, body) =
        blocking(move || post_audio(&format!("http://{addr}/api/transcribe"), wav(16))).await;
    assert_eq!(status, 502, "body: {body}");
    assert!(
        body.contains("no model loaded"),
        "the endpoint's own reason was dropped: {body}"
    );
}

/// A base URL pointed at something that is not a transcription server
/// answers with a web page. Treating that page as a transcript would put
/// HTML in the composer for a human to send to an agent.
#[tokio::test]
async fn a_reply_that_is_not_a_transcript_never_becomes_one() {
    let _env = ENV.lock().await;
    let _iso = Isolated::new();
    let addr = spawn().await;
    let upstream = Upstream::answering(200, "text/html", "<html><body>Welcome</body></html>");

    let base = upstream.base_url();
    blocking(move || {
        put(
            &format!("http://{addr}/api/settings"),
            &format!(r#"{{"transcribe_base_url":"{base}"}}"#),
        )
    })
    .await;

    let (status, body) =
        blocking(move || post_audio(&format!("http://{addr}/api/transcribe"), wav(16))).await;
    assert_eq!(status, 502, "body: {body}");
    assert!(!body.contains("Welcome"), "the page became a transcript");
}

#[tokio::test]
async fn a_transcription_endpoint_that_is_not_http_is_refused_at_save_time() {
    let _env = ENV.lock().await;
    let _iso = Isolated::new();
    let addr = spawn().await;

    let (status, body) = blocking(move || {
        put(
            &format!("http://{addr}/api/settings"),
            r#"{"transcribe_base_url":"file:///etc/passwd"}"#,
        )
    })
    .await;
    assert_eq!(status, 400, "body: {body}");

    let (_, settings) = blocking(move || {
        match ureq::get(&format!("http://{addr}/api/settings")).call() {
            Ok(r) => (r.status(), r.into_string().unwrap_or_default()),
            Err(e) => panic!("{e}"),
        }
    })
    .await;
    assert!(
        !settings.contains("/etc/passwd"),
        "the rejected URL was stored anyway: {settings}"
    );
}

/// The settings the UI reads to decide whether to offer a microphone, and
/// whether to warn that audio is leaving the machine.
#[tokio::test]
async fn settings_report_the_transcription_endpoint_and_whether_it_is_local() {
    let _env = ENV.lock().await;
    let _iso = Isolated::new();
    let addr = spawn().await;

    let (_, before) = blocking(move || {
        match ureq::get(&format!("http://{addr}/api/settings")).call() {
            Ok(r) => (r.status(), r.into_string().unwrap_or_default()),
            Err(e) => panic!("{e}"),
        }
    })
    .await;
    let json: serde_json::Value = serde_json::from_str(&before).expect("valid JSON");
    assert_eq!(json["transcribe_configured"], false, "body: {before}");

    blocking(move || {
        put(
            &format!("http://{addr}/api/settings"),
            r#"{"transcribe_base_url":"http://speech.example.com/v1"}"#,
        )
    })
    .await;

    let (_, after) = blocking(move || {
        match ureq::get(&format!("http://{addr}/api/settings")).call() {
            Ok(r) => (r.status(), r.into_string().unwrap_or_default()),
            Err(e) => panic!("{e}"),
        }
    })
    .await;
    let json: serde_json::Value = serde_json::from_str(&after).expect("valid JSON");
    assert_eq!(json["transcribe_configured"], true, "body: {after}");
    assert_eq!(
        json["transcribe_local"], false,
        "a remote endpoint was reported as local, body: {after}"
    );
    assert_eq!(json["transcribe_base_url_source"], "ui");
}
