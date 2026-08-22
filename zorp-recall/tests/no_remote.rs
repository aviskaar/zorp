//! Conversation text never leaves this device.
//!
//! This is the guarantee the whole crate exists to make, so it is asserted
//! from every direction a request could take: a redirect, a proxy, a
//! configured URL, and a resolver asked for a name nobody validated. Each
//! test points a would-be exfiltration at a loopback socket that counts
//! connections, and passes only when that count is zero.
//!
//! Counting connections rather than checking for an error is the point. A
//! request that failed and a request that was never made look identical
//! from the caller's side, and only one of them is the guarantee.

mod common;

use common::{dead_port, redirector, server, Canary};
use zorp_recall::{EmbedError, Embedder, LoopbackResolver, LoopbackUrl, OllamaEmbedder};

fn embedder(base: &str) -> OllamaEmbedder {
    let url = LoopbackUrl::parse(base).expect("test server is on loopback");
    OllamaEmbedder::new(url, "test-model")
}

/// A local server that answers 302 to somewhere else does not get to move
/// the request there. This is the hole a plain "the URL is loopback" check
/// leaves wide open: the URL was loopback, and the text went elsewhere.
#[test]
fn a_redirect_off_device_is_not_followed() {
    let canary = Canary::new();
    let base = redirector(&format!("{}/api/embeddings", canary.base));

    let err = embedder(&base)
        .embed("a private conversation about my salary")
        .expect_err("a redirect must not be followed");

    assert_eq!(canary.hits(), 0, "the redirect target was contacted");
    assert!(
        matches!(err, EmbedError::Redirected { .. }),
        "expected a refused redirect, got {err:?}"
    );
    assert!(
        err.to_string().contains("redirect"),
        "the refusal did not say why: {err}"
    );
}

/// The same, one step nastier: the redirect points at a name rather than an
/// address, so following it would mean a fresh DNS lookup that the guard
/// never saw.
#[test]
fn a_redirect_to_a_name_is_not_followed() {
    let base = redirector("https://api.openai.com/v1/embeddings");
    let err = embedder(&base).embed("a private conversation").unwrap_err();
    assert!(matches!(err, EmbedError::Redirected { .. }), "{err:?}");
}

/// The resolver is the last gate, and it is the one that holds when
/// something inside the HTTP client decides to connect somewhere the caller
/// did not name. It answers for the addresses the guard validated and for
/// nothing else, so a proxy, a redirect, or a middleware asking for
/// `api.openai.com:443` gets an error instead of a socket.
#[test]
fn the_resolver_answers_only_for_the_validated_addresses() {
    let url = LoopbackUrl::parse("http://127.0.0.1:11434").unwrap();
    let resolver = LoopbackResolver::for_url(&url);

    let allowed = ureq::Resolver::resolve(&resolver, "127.0.0.1:11434")
        .expect("the validated address must resolve");
    assert_eq!(allowed, url.addrs());

    for netloc in [
        "api.openai.com:443",
        "openrouter.ai:443",
        "8.8.8.8:11434",
        // Loopback, but not the port that was validated. A second local
        // service is still not the one the user pointed at.
        "127.0.0.1:9999",
        "localhost:11434",
    ] {
        assert!(
            ureq::Resolver::resolve(&resolver, netloc).is_err(),
            "{netloc} resolved through a guard that never validated it"
        );
    }
}

/// A URL that is not on this device is refused before any socket is opened,
/// and the refusal names the host so the message is actionable.
#[test]
fn an_off_device_url_is_refused_before_any_request() {
    for raw in [
        "https://api.openai.com/v1",
        "https://openrouter.ai/api/v1",
        "http://8.8.8.8:11434",
    ] {
        match OllamaEmbedder::at(raw, "test-model") {
            Ok(_) => panic!("{raw} was accepted as an embedding endpoint"),
            Err(err) => assert!(matches!(err, EmbedError::OffDevice(_)), "{raw}: {err:?}"),
        }
    }
}

/// No fallback. When the local embedder is not answering, the answer is an
/// error that says so, not a quiet hop to whatever else is configured.
/// `ZORP_BASE_URL` is the chat model's endpoint and is very often a real
/// remote API, so it is the specific thing that must not be reached for.
#[test]
fn an_unreachable_local_embedder_does_not_fall_back() {
    let canary = Canary::new();
    // Stand the canary up as the configured chat endpoint. If anything in
    // the embedding path ever reads it, this test fails.
    std::env::set_var("ZORP_BASE_URL", format!("{}/v1", canary.base));

    let err = embedder(&dead_port())
        .embed("a private conversation")
        .unwrap_err();

    assert_eq!(canary.hits(), 0, "the chat endpoint was contacted");
    assert!(matches!(err, EmbedError::Unreachable { .. }), "{err:?}");
    let message = err.to_string();
    assert!(
        message.contains("no local embedder"),
        "the refusal did not say the local embedder is missing: {message}"
    );
    std::env::remove_var("ZORP_BASE_URL");
}

/// A local server that answers with something other than an embedding is an
/// error too. Returning an empty vector would put a meaningless row in the
/// index and make search quietly wrong.
#[test]
fn a_malformed_answer_is_an_error_not_an_empty_vector() {
    for body in ["{}", "[]", "not json", r#"{"embedding": []}"#] {
        let (base, _rx) = server(200, body);
        let err = embedder(&base).embed("hello").unwrap_err();
        assert!(
            matches!(err, EmbedError::Malformed { .. }),
            "{body}: {err:?}"
        );
    }
}

/// The request carries the text, so it had better be going where it was
/// told. Pinned so a change of endpoint shape is a deliberate edit.
#[test]
fn the_request_goes_to_the_named_local_endpoint() {
    let (base, rx) = server(200, r#"{"embedding": [0.1, 0.2, 0.3]}"#);
    let vector = embedder(&base).embed("hello").expect("a local embedding");
    assert_eq!(vector.len(), 3);

    let request = common::captured(&rx);
    assert!(request.starts_with("POST /api/embeddings "), "{request}");
    assert!(request.contains("hello"), "{request}");
    // Nothing that could carry a credential to a third party.
    assert!(
        !request.to_lowercase().contains("authorization"),
        "the embedding request carried an Authorization header: {request}"
    );
}
