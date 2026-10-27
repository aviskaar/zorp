mod common;

use common::{dead_port, redirector, Canary};
use zorp_voice::{
    LoopbackResolver, LoopbackUrl, OllamaSpeech, QwenAsr, SpeechError, SpeechResolver, SpeechUrl,
    VoiceError,
};

fn client(base: &str) -> QwenAsr {
    QwenAsr::at(base, "test-model").unwrap()
}

#[test]
fn a_redirect_off_device_is_not_followed() {
    let canary = Canary::new();
    let base = redirector(&format!("{}/v1/chat/completions", canary.base));
    let err = client(&base)
        .transcribe(b"private voice", "audio/webm")
        .unwrap_err();
    assert_eq!(canary.hits(), 0, "the redirect target was contacted");
    assert!(matches!(err, VoiceError::Redirected { .. }), "{err:?}");
}

#[test]
fn an_off_device_url_is_refused_before_any_request() {
    for raw in [
        "https://api.openai.com/v1",
        "https://dashscope.aliyuncs.com/api/v1",
        "http://8.8.8.8:8000",
    ] {
        let err = QwenAsr::at(raw, "test-model").unwrap_err();
        assert!(matches!(err, VoiceError::OffDevice(_)), "{raw}: {err:?}");
    }
}

#[test]
fn the_resolver_answers_only_for_the_validated_host_and_port() {
    let url = LoopbackUrl::parse("http://127.0.0.1:8000").unwrap();
    let resolver = LoopbackResolver::for_url(&url);
    assert!(ureq::Resolver::resolve(&resolver, "127.0.0.1:8000").is_ok());
    for netloc in [
        "api.openai.com:443",
        "dashscope.aliyuncs.com:443",
        "127.0.0.1:9000",
        "localhost:8000",
    ] {
        assert!(
            ureq::Resolver::resolve(&resolver, netloc).is_err(),
            "{netloc} escaped the pinned resolver"
        );
    }
}

// The TTS half has the same boundary as the ASR half: text that may quote a
// person's files goes to a loopback address or nowhere. These are the same
// four-layer proofs, counting connections to a canary rather than trusting an
// error message, because a request never made and a request that failed look
// the same from the caller's side.

#[test]
fn speech_refuses_an_off_device_url_before_any_request() {
    for raw in [
        "https://api.openai.com/v1",
        "https://api.elevenlabs.io",
        "http://8.8.8.8:11434",
    ] {
        let err = OllamaSpeech::at(raw, "phonon").unwrap_err();
        assert!(matches!(err, SpeechError::OffDevice(_)), "{raw}: {err:?}");
    }
}

#[test]
fn a_speech_redirect_off_device_is_not_followed() {
    let canary = Canary::new();
    let base = redirector(&format!("{}/v1/audio/speech", canary.base));
    let err = OllamaSpeech::at(&base, "phonon")
        .unwrap()
        .speak("a private answer quoting files", "phonon", "default")
        .unwrap_err();
    assert_eq!(canary.hits(), 0, "the redirect target was contacted");
    assert!(matches!(err, SpeechError::Redirected { .. }), "{err:?}");
}

#[test]
fn the_speech_resolver_answers_only_for_the_validated_host_and_port() {
    let url = SpeechUrl::parse("http://127.0.0.1:11434").unwrap();
    let resolver = SpeechResolver::for_url(&url);
    assert!(ureq::Resolver::resolve(&resolver, "127.0.0.1:11434").is_ok());
    for netloc in [
        "api.openai.com:443",
        "api.elevenlabs.io:443",
        "127.0.0.1:9000",
        "localhost:11434",
    ] {
        assert!(
            ureq::Resolver::resolve(&resolver, netloc).is_err(),
            "{netloc} escaped the pinned resolver"
        );
    }
}

#[test]
fn an_unreachable_speech_runtime_does_not_fall_back() {
    let canary = Canary::new();
    std::env::set_var("ZORP_BASE_URL", &canary.base);
    let status = OllamaSpeech::at(&dead_port(), "phonon").unwrap().status();
    std::env::remove_var("ZORP_BASE_URL");
    assert!(!status.runtime_reachable);
    assert_eq!(
        canary.hits(),
        0,
        "a configured remote endpoint was contacted"
    );
}

#[test]
fn an_unreachable_runtime_does_not_fall_back() {
    let canary = Canary::new();
    std::env::set_var("ZORP_BASE_URL", &canary.base);
    std::env::set_var("DASHSCOPE_BASE_URL", &canary.base);
    let status = client(&dead_port()).status();
    std::env::remove_var("ZORP_BASE_URL");
    std::env::remove_var("DASHSCOPE_BASE_URL");
    assert!(!status.runtime_reachable);
    assert_eq!(
        canary.hits(),
        0,
        "a configured remote endpoint was contacted"
    );
}
