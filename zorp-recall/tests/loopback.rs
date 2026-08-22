//! The guard that decides whether an address is on this device.
//!
//! This is the most important test file in the crate. Everything else here
//! is a convenience; this is the thing standing between a person's entire
//! chat history and somebody else's server. Each case below is a URL that
//! must be refused, and the reason it would otherwise get through.

use zorp_recall::{LoopbackError, LoopbackUrl};

/// Addresses that are plainly somewhere else.
#[test]
fn a_public_address_is_refused() {
    for raw in [
        "https://api.openai.com/v1",
        "https://openrouter.ai/api/v1",
        "http://8.8.8.8:11434",
        "http://93.184.216.34/v1/embeddings",
        "http://[2001:4860:4860::8888]:11434",
    ] {
        let refused = LoopbackUrl::parse(raw);
        assert!(refused.is_err(), "{raw} was accepted as on-device");
    }
}

/// The near misses. Every one of these contains a loopback address as a
/// substring, which is exactly why a substring check is not the guard.
#[test]
fn a_name_that_merely_looks_like_loopback_is_refused() {
    for raw in [
        "http://127.0.0.1.evil.example/v1",
        "http://localhost.evil.example/v1",
        "http://evil.example/127.0.0.1",
        "http://user@127.0.0.1:11434/v1",
        "http://127.0.0.1:11434@evil.example/v1",
    ] {
        let refused = LoopbackUrl::parse(raw);
        assert!(refused.is_err(), "{raw} was accepted as on-device");
    }
}

/// 0.0.0.0 is not a loopback address. On some platforms connecting to it
/// reaches this machine anyway, which is precisely the kind of "it works,
/// so it must be fine" that this guard exists to not rely on.
#[test]
fn the_unspecified_address_is_refused() {
    assert!(LoopbackUrl::parse("http://0.0.0.0:11434").is_err());
    assert!(LoopbackUrl::parse("http://[::]:11434").is_err());
}

/// A scheme that is not HTTP is refused rather than handed to a library to
/// interpret. `file:` and `ftp:` are not embedding endpoints, and a scheme
/// nobody thought about is not a scheme to allow by default.
#[test]
fn a_non_http_scheme_is_refused() {
    for raw in [
        "file:///etc/passwd",
        "ftp://127.0.0.1/",
        "127.0.0.1:11434",
        "//127.0.0.1:11434",
        "",
        "   ",
    ] {
        assert!(
            LoopbackUrl::parse(raw).is_err(),
            "{raw:?} was accepted as on-device"
        );
    }
}

/// What must be allowed, or the feature does not work at all.
#[test]
fn the_local_forms_are_accepted() {
    for raw in [
        "http://127.0.0.1:11434",
        "http://127.0.0.1:11434/",
        "http://127.1.2.3:11434/v1",
        "http://localhost:11434",
        "http://[::1]:11434/v1",
        // An IPv4-mapped IPv6 loopback really is loopback. `is_loopback` on
        // `Ipv6Addr` says no, so the guard has to unmap before it asks.
        "http://[::ffff:127.0.0.1]:11434",
        "https://127.0.0.1:11434",
    ] {
        assert!(
            LoopbackUrl::parse(raw).is_ok(),
            "{raw} was refused but is on this device"
        );
    }
}

/// The guard resolves the name and keeps the answer. Anything that connects
/// later connects to these and to nothing else, so a name that resolves to
/// two addresses, one of them off-device, is refused whole rather than
/// filtered down to the safe half.
#[test]
fn an_accepted_url_carries_only_loopback_addresses() {
    let url = LoopbackUrl::parse("http://localhost:11434").expect("localhost is on this device");
    assert!(!url.addrs().is_empty(), "no address was resolved");
    for addr in url.addrs() {
        assert!(addr.ip().is_loopback(), "{addr} is not loopback");
        assert_eq!(addr.port(), 11434);
    }
}

/// The default when nothing is configured is Ollama on loopback, and it
/// passes its own guard. A default that the guard refuses would mean the
/// feature is off for everyone until they set a variable.
#[test]
fn the_default_endpoint_is_on_device() {
    assert!(LoopbackUrl::parse(zorp_recall::DEFAULT_EMBED_URL).is_ok());
}

/// Refusals name the host, because "embedding is unavailable" with no
/// reason is the message that gets worked around rather than fixed.
#[test]
fn a_refusal_says_what_it_refused() {
    let err = LoopbackUrl::parse("https://api.openai.com/v1").unwrap_err();
    let message = err.to_string();
    assert!(
        message.contains("api.openai.com"),
        "refusal did not name the host: {message}"
    );
    assert!(matches!(err, LoopbackError::OffDevice { .. }));
}
