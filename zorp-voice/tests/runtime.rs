mod common;

use common::{captured, server};
use zorp_voice::{QwenAsr, DEFAULT_VOICE_MODEL};

#[test]
fn status_observes_health_and_the_loaded_model() {
    let models = format!(r#"{{"object":"list","data":[{{"id":"{DEFAULT_VOICE_MODEL}"}}]}}"#);
    let (base, rx) = server(vec![
        ("application/json", r#"{"status":"ok"}"#),
        ("application/json", &models),
    ]);
    let client = QwenAsr::at(&base, DEFAULT_VOICE_MODEL).unwrap();

    let status = client.status();

    assert!(status.runtime_reachable, "{status:?}");
    assert!(status.model_present, "{status:?}");
    assert!(captured(&rx).starts_with("GET /health "));
    assert!(captured(&rx).starts_with("GET /v1/models "));
}

#[test]
fn transcription_uses_qwen_chat_audio_and_returns_the_detected_language() {
    let (base, rx) = server(vec![(
        "application/json",
        r#"{"choices":[{"message":{"content":"language हिन्दी<asr_text>नमस्ते दुनिया"}}]}"#,
    )]);
    let client = QwenAsr::at(&base, DEFAULT_VOICE_MODEL).unwrap();

    let transcript = client.transcribe(b"not-real-audio", "audio/webm").unwrap();

    assert_eq!(transcript.language, "हिन्दी");
    assert_eq!(transcript.text, "नमस्ते दुनिया");
    let request = captured(&rx);
    assert!(
        request.starts_with("POST /v1/chat/completions "),
        "{request}"
    );
    assert!(request.contains("application/json"), "{request}");
    assert!(request.contains(DEFAULT_VOICE_MODEL), "{request}");
    assert!(request.contains("audio_url"), "{request}");
    assert!(
        request.contains("data:audio/webm;base64,bm90LXJlYWwtYXVkaW8="),
        "{request}"
    );
    assert!(!request.to_ascii_lowercase().contains("authorization"));
    assert!(!request.contains("English"));
}

#[test]
fn a_chat_answer_without_qwen_content_is_rejected() {
    let (base, _rx) = server(vec![("application/json", r#"{"choices":[]}"#)]);
    let client = QwenAsr::at(&base, DEFAULT_VOICE_MODEL).unwrap();
    let err = client.transcribe(b"audio", "audio/ogg").unwrap_err();
    assert!(err.to_string().contains("message content"), "{err}");
}
