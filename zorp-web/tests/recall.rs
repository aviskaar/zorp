//! Searching your own conversations, end to end and on this machine.
//!
//! The embedding server used here is a few lines of `TcpListener` answering
//! Ollama's wire shape with a topic vector. It is a stub, but it is a stub
//! on the other end of a real socket, so what these tests exercise is the
//! whole path: read the store, chunk it, embed over loopback HTTP, write
//! SQLite, embed a query, rank, answer JSON.
//!
//! Two of them are about what happens when there is no model to talk to,
//! because that is the case where a feature like this is tempted to reach
//! for a remote API, and it must not.
//!
//! Everything below the shared helpers lives inside the feature, including
//! the fixtures, so a build without `recall` compiles this file down to the
//! one test that says the feature is off.

use std::net::SocketAddr;

async fn spawn() -> SocketAddr {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, zorp_web::api::router())
            .await
            .unwrap();
    });
    addr
}

async fn get(url: String) -> (u16, String) {
    tokio::task::spawn_blocking(move || match ureq::get(&url).call() {
        Ok(r) => (r.status(), r.into_string().unwrap_or_default()),
        Err(ureq::Error::Status(code, r)) => (code, r.into_string().unwrap_or_default()),
        Err(e) => panic!("{e}"),
    })
    .await
    .unwrap()
}

async fn post(url: String) -> (u16, String) {
    tokio::task::spawn_blocking(move || {
        match ureq::post(&url)
            .set("content-type", "application/json")
            .send_string("{}")
        {
            Ok(r) => (r.status(), r.into_string().unwrap_or_default()),
            Err(ureq::Error::Status(code, r)) => (code, r.into_string().unwrap_or_default()),
            Err(e) => panic!("{e}"),
        }
    })
    .await
    .unwrap()
}

fn json(body: &str) -> serde_json::Value {
    serde_json::from_str(body).unwrap_or_else(|_| panic!("not JSON: {body}"))
}

/// The feature is compiled out unless it is asked for, so the endpoints
/// still answer, and they say why they are not going to do anything.
#[cfg(not(feature = "recall"))]
#[tokio::test]
async fn search_is_off_unless_the_feature_is_on() {
    let addr = spawn().await;
    let (status, body) = get(format!("http://{addr}/api/recall/status")).await;
    assert_eq!(status, 200);
    let body = json(&body);
    assert_eq!(body["available"], false);
    assert!(
        body["reason"]
            .as_str()
            .unwrap_or_default()
            .contains("recall"),
        "the reason did not name the feature: {body}"
    );

    let (status, _) = get(format!("http://{addr}/api/recall/search?q=refund")).await;
    assert_eq!(status, 501);
    let (status, _) = post(format!("http://{addr}/api/recall/index")).await;
    assert_eq!(status, 501);
}

#[cfg(feature = "recall")]
mod on {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::path::Path;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use std::thread;
    use tokio::sync::Mutex;

    /// `ZORP_STATE_DB`, `ZORP_RECALL_DB` and `ZORP_EMBED_URL` are process
    /// wide, so tests that set them take turns.
    static ENV: Mutex<()> = Mutex::const_new(());

    /// Three conversations, on three unrelated subjects.
    fn seed(db: &Path) {
        use zorp_agent::{Message, Store};
        let mut store = Store::open_at(db).unwrap();
        let conversations = [
            (
                "conv-money",
                "Sorting out the account",
                "the customer wants a refund on last month's charge",
                "I can look at the payment history for you",
            ),
            (
                "conv-dog",
                "Walking the dog",
                "the retriever chewed through her leash again",
                "a kennel might help while you are out",
            ),
            (
                "conv-rust",
                "Borrow checker trouble",
                "cargo will not build, the borrow outlives the crate",
                "the lifetime needs to be named properly",
            ),
        ];
        for (id, task, ask, answer) in conversations {
            store.create_session(id, task, "repo", "model").unwrap();
            store.record_message(id, 0, &Message::user(ask)).unwrap();
            store
                .record_message(id, 1, &Message::assistant(answer))
                .unwrap();
        }
    }

    /// A loopback server speaking Ollama's `/api/embeddings` shape,
    /// answering a four-axis topic vector. Deterministic, so a ranking
    /// assertion is a statement about the search and not about a model's
    /// mood.
    fn embedding_server() -> String {
        const TOPICS: [&[&str]; 4] = [
            &["invoice", "billing", "refund", "charge", "payment", "price"],
            &["dog", "puppy", "retriever", "leash", "kennel"],
            &["rust", "cargo", "crate", "borrow", "lifetime"],
            &["sleep", "insomnia", "nap", "bedtime"],
        ];
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let mut stream = stream;
                let request = read_request(&mut stream);
                let prompt = request
                    .split_once("\r\n\r\n")
                    .and_then(|(_, body)| serde_json::from_str::<serde_json::Value>(body).ok())
                    .and_then(|v| v.get("prompt").and_then(|p| p.as_str()).map(str::to_string))
                    .unwrap_or_default()
                    .to_lowercase();
                let mut vector = vec![0.0f64; TOPICS.len()];
                for (axis, words) in TOPICS.iter().enumerate() {
                    for word in *words {
                        if prompt.contains(word) {
                            vector[axis] += 1.0;
                        }
                    }
                }
                if vector.iter().all(|x| *x == 0.0) {
                    vector[0] = 0.01;
                    vector[1] = -0.01;
                }
                let body = serde_json::json!({ "embedding": vector }).to_string();
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = stream.write_all(response.as_bytes());
                let _ = stream.flush();
                let _ = stream.shutdown(std::net::Shutdown::Write);
            }
        });
        format!("http://{addr}")
    }

    /// A socket that must never be connected to, and the count that proves
    /// it was not.
    struct Canary {
        base: String,
        hits: Arc<AtomicUsize>,
    }

    impl Canary {
        fn new() -> Canary {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let addr = listener.local_addr().unwrap();
            let hits = Arc::new(AtomicUsize::new(0));
            let counter = Arc::clone(&hits);
            thread::spawn(move || {
                for stream in listener.incoming() {
                    counter.fetch_add(1, Ordering::SeqCst);
                    if let Ok(stream) = stream {
                        let _ = stream.shutdown(std::net::Shutdown::Both);
                    }
                }
            });
            Canary {
                base: format!("http://{addr}"),
                hits,
            }
        }

        fn hits(&self) -> usize {
            thread::sleep(std::time::Duration::from_millis(150));
            self.hits.load(Ordering::SeqCst)
        }
    }

    /// A loopback address with nothing listening on it.
    fn dead_port() -> String {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        drop(listener);
        format!("http://{addr}")
    }

    fn read_request(stream: &mut std::net::TcpStream) -> String {
        let mut buf = Vec::new();
        let mut chunk = [0u8; 1024];
        loop {
            let n = stream.read(&mut chunk).unwrap_or(0);
            if n == 0 {
                break;
            }
            buf.extend_from_slice(&chunk[..n]);
            if let Some(header) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                let end = header + 4;
                let headers = String::from_utf8_lossy(&buf[..end]);
                let length = headers
                    .lines()
                    .find_map(|line| {
                        let (key, value) = line.split_once(':')?;
                        key.trim()
                            .eq_ignore_ascii_case("content-length")
                            .then(|| value.trim().parse::<usize>().ok())
                            .flatten()
                    })
                    .unwrap_or(0);
                while buf.len() < end + length {
                    let n = stream.read(&mut chunk).unwrap_or(0);
                    if n == 0 {
                        break;
                    }
                    buf.extend_from_slice(&chunk[..n]);
                }
                break;
            }
        }
        String::from_utf8_lossy(&buf).into_owned()
    }

    /// A scratch store and a scratch index, both thrown away with the
    /// directory. Nothing here ever opens the real one.
    struct Fixture {
        dir: tempfile::TempDir,
    }

    impl Fixture {
        fn sessions_db(&self) -> std::path::PathBuf {
            self.dir.path().join("sessions.db")
        }
    }

    fn set_up(embed_url: Option<&str>) -> Fixture {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("sessions.db");
        std::env::set_var("ZORP_STATE_DB", &db);
        std::env::set_var("ZORP_RECALL_DB", dir.path().join("recall.db"));
        match embed_url {
            Some(url) => std::env::set_var("ZORP_EMBED_URL", url),
            None => std::env::remove_var("ZORP_EMBED_URL"),
        }
        seed(&db);
        Fixture { dir }
    }

    /// The whole point. "invoice dispute" is in none of the conversations,
    /// letter for letter, and finds the one about a refund.
    #[tokio::test]
    async fn search_finds_a_conversation_no_substring_match_would() {
        let _env = ENV.lock().await;
        let _fixture = set_up(Some(&embedding_server()));
        let addr = spawn().await;

        let (status, body) = post(format!("http://{addr}/api/recall/index")).await;
        assert_eq!(status, 200, "{body}");
        let report = json(&body);
        assert_eq!(report["indexed"], 3, "{report}");
        assert_eq!(report["chunks"], 6, "{report}");

        let (status, body) = get(format!(
            "http://{addr}/api/recall/search?q=invoice%20dispute&limit=3"
        ))
        .await;
        assert_eq!(status, 200, "{body}");
        let payload = json(&body);
        let hits = payload["hits"].as_array().unwrap();
        assert!(!hits.is_empty(), "no results");
        assert_eq!(hits[0]["id"], "conv-money");
        assert_eq!(hits[0]["title"], "Sorting out the account");
        // The snippet is the message that matched, so a result is worth
        // looking at before you click it.
        assert!(hits[0]["snippet"].as_str().unwrap().contains("refund"));
    }

    /// The second run does no work. Reindexing every conversation from
    /// scratch each time the button is pressed would make the button
    /// something nobody presses.
    #[tokio::test]
    async fn a_second_index_skips_what_has_not_changed() {
        let _env = ENV.lock().await;
        let _fixture = set_up(Some(&embedding_server()));
        let addr = spawn().await;

        let (_, body) = post(format!("http://{addr}/api/recall/index")).await;
        assert_eq!(json(&body)["indexed"], 3);

        let (_, body) = post(format!("http://{addr}/api/recall/index")).await;
        let report = json(&body);
        assert_eq!(report["indexed"], 0, "{report}");
        assert_eq!(report["skipped"], 3, "{report}");
    }

    /// Status reports what is configured and how much is indexed, so the
    /// page can say "nothing indexed yet" instead of "no results".
    #[tokio::test]
    async fn status_reports_the_endpoint_and_the_index_size() {
        let _env = ENV.lock().await;
        let embedder = embedding_server();
        let _fixture = set_up(Some(&embedder));
        let addr = spawn().await;

        let (_, body) = get(format!("http://{addr}/api/recall/status")).await;
        let before = json(&body);
        assert_eq!(before["available"], true, "{before}");
        assert_eq!(before["conversations"], 0);
        assert_eq!(before["endpoint"], embedder);

        post(format!("http://{addr}/api/recall/index")).await;
        let (_, body) = get(format!("http://{addr}/api/recall/status")).await;
        let after = json(&body);
        assert_eq!(after["conversations"], 3, "{after}");
        assert_eq!(after["chunks"], 6, "{after}");
    }

    /// No local embedder, no search, no quiet hop to a remote API. The
    /// canary stands in for the configured chat endpoint, which is very
    /// often a real remote one, and it must not be touched.
    #[tokio::test]
    async fn no_local_embedder_means_an_explicit_refusal() {
        let _env = ENV.lock().await;
        let canary = Canary::new();
        let _fixture = set_up(Some(&dead_port()));
        std::env::set_var("ZORP_BASE_URL", format!("{}/v1", canary.base));
        let addr = spawn().await;

        let (status, body) = post(format!("http://{addr}/api/recall/index")).await;
        assert_eq!(status, 503, "{body}");
        assert!(
            body.contains("no local embedder"),
            "the refusal did not say what is missing: {body}"
        );

        let (status, body) = get(format!("http://{addr}/api/recall/search?q=refund")).await;
        assert_eq!(status, 503, "{body}");
        assert!(body.contains("no local embedder"), "{body}");

        std::env::remove_var("ZORP_BASE_URL");
        assert_eq!(canary.hits(), 0, "the chat endpoint was contacted");
    }

    /// Pointing the embedder at a remote host does not get you a remote
    /// embedder. It gets you a refusal that names the host.
    #[tokio::test]
    async fn a_remote_embed_url_is_refused() {
        let _env = ENV.lock().await;
        let _fixture = set_up(Some("https://api.openai.com/v1"));
        let addr = spawn().await;

        let (status, body) = get(format!("http://{addr}/api/recall/status")).await;
        assert_eq!(status, 200);
        let reported = json(&body);
        assert_eq!(reported["available"], false, "{reported}");
        assert!(
            reported["reason"]
                .as_str()
                .unwrap_or_default()
                .contains("api.openai.com"),
            "{reported}"
        );

        let (status, body) = post(format!("http://{addr}/api/recall/index")).await;
        assert_eq!(status, 503, "{body}");
        assert!(body.contains("api.openai.com"), "{body}");
    }

    /// An empty query is not a search. It would embed the empty string and
    /// rank the whole corpus against nothing.
    #[tokio::test]
    async fn an_empty_query_is_refused() {
        let _env = ENV.lock().await;
        let _fixture = set_up(Some(&embedding_server()));
        let addr = spawn().await;
        let (status, _) = get(format!("http://{addr}/api/recall/search?q=%20%20")).await;
        assert_eq!(status, 400);
    }

    /// A conversation deleted from the store stops being a search result.
    #[tokio::test]
    async fn indexing_drops_conversations_the_store_no_longer_has() {
        let _env = ENV.lock().await;
        let fixture = set_up(Some(&embedding_server()));
        let addr = spawn().await;
        post(format!("http://{addr}/api/recall/index")).await;

        // Not the user's store: the scratch one in this test's own
        // directory, which the fixture deletes when it drops.
        rusqlite::Connection::open(fixture.sessions_db())
            .unwrap()
            .execute("DELETE FROM sessions WHERE id = 'conv-dog'", [])
            .unwrap();

        let (_, body) = post(format!("http://{addr}/api/recall/index")).await;
        assert_eq!(json(&body)["removed"], 1, "{body}");

        let (_, body) = get(format!("http://{addr}/api/recall/search?q=puppy")).await;
        let payload = json(&body);
        assert!(payload["hits"]
            .as_array()
            .unwrap()
            .iter()
            .all(|h| h["id"] != "conv-dog"));
    }
}
