//! Speech routes are present in every build and only talk to the checked
//! `zorp-voice` client when the opt-in `voice` feature is compiled.

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
    match request.send_bytes(body) {
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
async fn routes_explain_that_the_feature_is_off() {
    let addr = spawn().await;
    let (status, body) = tokio::task::spawn_blocking(move || {
        request(
            "GET",
            &format!("http://{addr}/api/speech/status"),
            None,
            &[],
        )
    })
    .await
    .unwrap();
    assert_eq!(status, 200, "{body}");
    let value: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(value["available"], false);
    assert!(value["detail"].as_str().unwrap().contains("voice feature"));

    // The mutating routes answer 501 so the page can say why speech is off.
    let url = format!("http://{addr}/api/speech");
    let (status, _) = tokio::task::spawn_blocking(move || {
        request("POST", &url, Some("application/json"), br#"{"text":"hi"}"#)
    })
    .await
    .unwrap();
    assert_eq!(status, 501);
}

#[cfg(feature = "voice")]
#[tokio::test]
async fn chunks_splits_text_on_sentence_boundaries() {
    let addr = spawn().await;
    let url = format!("http://{addr}/api/speech/chunks");
    let body = serde_json::json!({
        "text": "One sentence here. Two sentence here. Three sentence here."
    })
    .to_string();
    let (status, body) = tokio::task::spawn_blocking(move || {
        request("POST", &url, Some("application/json"), body.as_bytes())
    })
    .await
    .unwrap();
    assert_eq!(status, 200, "{body}");
    let value: serde_json::Value = serde_json::from_str(&body).unwrap();
    let chunks = value["chunks"].as_array().unwrap();
    assert!(!chunks.is_empty());
    // Every word survives the split.
    let joined: String = chunks
        .iter()
        .map(|c| c.as_str().unwrap())
        .collect::<Vec<_>>()
        .join(" ");
    for word in ["One", "Two", "Three"] {
        assert!(joined.contains(word), "{joined}");
    }
}

#[cfg(feature = "voice")]
#[tokio::test]
async fn an_empty_speak_body_is_rejected_without_reaching_a_runtime() {
    let addr = spawn().await;
    let url = format!("http://{addr}/api/speech");
    let (status, _) = tokio::task::spawn_blocking(move || {
        request("POST", &url, Some("application/json"), br#"{"text":"   "}"#)
    })
    .await
    .unwrap();
    assert_eq!(status, 400);
}
