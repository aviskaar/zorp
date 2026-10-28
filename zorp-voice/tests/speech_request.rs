mod common;

use common::{captured, server};
use zorp_voice::OllamaSpeech;

/// `speak` posts to `/v1/audio/speech` with the model, the text as `input`,
/// and the chosen voice, and hands back the bytes and the runtime's own
/// content type. A guessed body is exactly how a request fails with a bare
/// transport error, which is why this checks the body the client sends.
#[test]
fn speak_sends_the_openai_speech_body_and_returns_audio() {
    let (base, rx) = server(vec![("audio/mpeg", "ID3fake-audio-bytes")]);
    let client = OllamaSpeech::at(&base, "phonon").unwrap();
    let audio = client.speak("Hello there.", "phonon", "river").unwrap();

    let request = captured(&rx);
    assert!(request.contains("POST /v1/audio/speech"), "{request}");
    assert!(request.contains("\"model\":\"phonon\""), "{request}");
    assert!(request.contains("\"input\":\"Hello there.\""), "{request}");
    assert!(request.contains("\"voice\":\"river\""), "{request}");
    assert_eq!(audio.media_type, "audio/mpeg");
    assert_eq!(audio.bytes, b"ID3fake-audio-bytes");
}

/// `voices` posts the model to `/api/show` and reads the list back.
#[test]
fn voices_reads_the_list_from_api_show() {
    let (base, rx) = server(vec![(
        "application/json",
        r#"{"model_info":{"phonon.voices":["default","river"]}}"#,
    )]);
    let client = OllamaSpeech::at(&base, "phonon").unwrap();
    let voices = client.voices("phonon").unwrap();

    let request = captured(&rx);
    assert!(request.contains("POST /api/show"), "{request}");
    assert!(request.contains("\"model\":\"phonon\""), "{request}");
    assert_eq!(voices, vec!["default", "river"]);
}
