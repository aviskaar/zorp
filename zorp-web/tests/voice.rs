//! Voice routes are present in every build and only send audio through the
//! checked `zorp-voice` client when the opt-in feature is compiled.

use std::net::SocketAddr;
use zorp_web::state::AppState;

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

fn request(method: &str, url: &str, content_type: Option<&str>, body: &[u8]) -> (u16, String) {
    let mut request = ureq::request(method, url);
    if let Some(content_type) = content_type {
        request = request.set("content-type", content_type);
    }
    let result = request.send_bytes(body);
    match result {
        Ok(response) => (
            response.status(),
            response.into_string().unwrap_or_default(),
        ),
        Err(ureq::Error::Status(status, response)) => {
            (status, response.into_string().unwrap_or_default())
        }
        Err(error) => panic!("{error}"),
    }
}

#[cfg(not(feature = "voice"))]
#[tokio::test]
async fn routes_explain_that_the_voice_feature_is_off() {
    let addr = spawn().await;
    let (status, body) = tokio::task::spawn_blocking(move || {
        request("GET", &format!("http://{addr}/api/voice/status"), None, &[])
    })
    .await
    .unwrap();
    assert_eq!(status, 200, "{body}");
    let value: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(value["available"], false);
    assert!(value["detail"].as_str().unwrap().contains("voice feature"));

    for path in ["wait", "transcribe"] {
        let url = format!("http://{addr}/api/voice/{path}");
        let (status, body) = tokio::task::spawn_blocking(move || {
            request("POST", &url, Some("audio/webm"), b"audio")
        })
        .await
        .unwrap();
        assert_eq!(status, 501, "{path}: {body}");
    }
}

#[cfg(feature = "voice")]
mod enabled {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use tokio::sync::Mutex;

    static ENV: Mutex<()> = Mutex::const_new(());

    fn runtime(responses: Vec<(&str, &str)>) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let responses: Vec<(String, String)> = responses
            .into_iter()
            .map(|(content_type, body)| (content_type.into(), body.into()))
            .collect();
        std::thread::spawn(move || {
            for (content_type, body) in responses {
                let (mut stream, _) = listener.accept().unwrap();
                let mut bytes = [0u8; 16384];
                let _ = stream.read(&mut bytes);
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                stream.write_all(response.as_bytes()).unwrap();
            }
        });
        format!("http://{addr}")
    }

    #[tokio::test]
    async fn status_and_capability_share_one_observed_shape() {
        let _guard = ENV.lock().await;
        let model = zorp_voice::DEFAULT_VOICE_MODEL;
        let models = format!(r#"{{"data":[{{"id":"{model}"}}]}}"#);
        let base = runtime(vec![
            ("application/json", r#"{"status":"ok"}"#),
            ("application/json", &models),
            ("application/json", r#"{"status":"ok"}"#),
            ("application/json", &models),
        ]);
        std::env::set_var("ZORP_VOICE_URL", &base);
        let addr = spawn().await;
        let status_url = format!("http://{addr}/api/voice/status");
        let capabilities_url = format!("http://{addr}/api/capabilities");
        let (status_code, status_body) =
            tokio::task::spawn_blocking(move || request("GET", &status_url, None, &[]))
                .await
                .unwrap();
        let (_, capabilities_body) =
            tokio::task::spawn_blocking(move || request("GET", &capabilities_url, None, &[]))
                .await
                .unwrap();
        std::env::remove_var("ZORP_VOICE_URL");
        assert_eq!(status_code, 200, "{status_body}");
        let status: serde_json::Value = serde_json::from_str(&status_body).unwrap();
        let capabilities: serde_json::Value = serde_json::from_str(&capabilities_body).unwrap();
        assert_eq!(capabilities["voice"], status);
        assert_eq!(status["runtime_reachable"], true);
        assert_eq!(status["model_present"], true);
        assert!(status["command"]
            .as_str()
            .unwrap()
            .contains("qwen-asr-serve"));
    }

    #[tokio::test]
    async fn transcription_returns_editable_text_and_language() {
        let _guard = ENV.lock().await;
        let base = runtime(vec![(
            "application/json",
            r#"{"choices":[{"message":{"content":"language español<asr_text>hola mundo"}}]}"#,
        )]);
        std::env::set_var("ZORP_VOICE_URL", &base);
        let addr = spawn().await;
        let url = format!("http://{addr}/api/voice/transcribe");
        let (status, body) = tokio::task::spawn_blocking(move || {
            request("POST", &url, Some("audio/webm"), b"recorded bytes")
        })
        .await
        .unwrap();
        std::env::remove_var("ZORP_VOICE_URL");
        assert_eq!(status, 200, "{body}");
        let value: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(value["text"], "hola mundo");
        assert_eq!(value["language"], "español");
    }

    #[tokio::test]
    async fn wait_stream_polls_observed_runtime_readiness() {
        let _guard = ENV.lock().await;
        let model = zorp_voice::DEFAULT_VOICE_MODEL;
        let models = format!(r#"{{"data":[{{"id":"{model}"}}]}}"#);
        let base = runtime(vec![
            ("application/json", "{}"),
            ("application/json", &models),
        ]);
        std::env::set_var("ZORP_VOICE_URL", &base);
        let addr = spawn().await;
        let url = format!("http://{addr}/api/voice/wait");
        let (status, body) = tokio::task::spawn_blocking(move || {
            request("POST", &url, Some("application/json"), b"{}")
        })
        .await
        .unwrap();
        std::env::remove_var("ZORP_VOICE_URL");
        assert_eq!(status, 200, "{body}");
        assert!(body.contains("\"status\":\"ready\""), "{body}");
        assert!(!body.contains("progress"), "{body}");
    }

    #[tokio::test]
    async fn wait_rejects_a_bodyless_simple_post() {
        let _guard = ENV.lock().await;
        let addr = spawn().await;
        let url = format!("http://{addr}/api/voice/wait");
        let (status, body) = tokio::task::spawn_blocking(move || request("POST", &url, None, &[]))
            .await
            .unwrap();
        assert_eq!(status, 415, "{body}");
        assert!(body.contains("application/json"), "{body}");
    }
}
