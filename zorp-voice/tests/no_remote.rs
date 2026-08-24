mod common;

use common::{dead_port, redirector, Canary};
use zorp_voice::{LoopbackResolver, LoopbackUrl, QwenAsr, VoiceError};

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
