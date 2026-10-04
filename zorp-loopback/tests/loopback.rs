//! The guard that decides whether an address is on this device.
//!
//! This is the most important test file in the workspace's two loopback
//! crates. Everything else in `zorp-recall` and `zorp-voice` is a
//! convenience; this is the thing standing between a person's entire chat
//! history, or a recording of their voice, and somebody else's server. Each
//! case below is a URL that must be refused, and the reason it would
//! otherwise get through.
//!
//! These run against the one copy of the guard, with a wording of their
//! own. How each caller's refusals read is pinned in that caller's
//! `tests/loopback.rs`; whether anything actually leaves the machine is
//! pinned by each caller's `tests/no_remote.rs` and `tests/no_proxy.rs`,
//! which count connections and stay with the HTTP agents they test.

use zorp_loopback::{LoopbackError, Phrases, Wording};

#[derive(Debug, Clone, Copy)]
struct Test;

impl Wording for Test {
    const PHRASES: &'static Phrases = &Phrases {
        endpoint: "test endpoint",
        not_scheme_for: "a test endpoint",
        refusing_to: "send test bytes to",
        only_ever: "Test bytes stay here",
        pinned_before: "tests are pinned to ",
        pinned_after: ", and nowhere else",
    };
}

type LoopbackUrl = zorp_loopback::LoopbackUrl<Test>;
type LoopbackResolver = zorp_loopback::LoopbackResolver<Test>;

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

/// The near misses on the IPv6 side. Unmapping is only allowed to say yes
/// to an IPv4 address that is itself loopback.
#[test]
fn a_mapped_or_bracketed_public_address_is_refused() {
    for raw in [
        "http://[::ffff:8.8.8.8]:11434",
        "http://[::ffff:0.0.0.0]:11434",
        "http://[fe80::1]:11434",
    ] {
        assert!(
            LoopbackUrl::parse(raw).is_err(),
            "{raw} was accepted as on-device"
        );
    }
}

/// A port nobody can read the same way twice is refused, not defaulted.
/// So is a bracket that does not close, or anything after one that is not
/// a port. Each names the URL and the reason.
#[test]
fn an_unreadable_authority_is_refused_with_its_reason() {
    for (raw, reason) in [
        ("http://", "it names no host"),
        ("http:///v1", "it names no host"),
        ("http://[::1", "its bracket is unclosed"),
        ("http://[::1]x", "it has junk after the bracket"),
        ("http://[::1]:x", "its port is not a number"),
        ("http://127.0.0.1:x", "its port is not a number"),
        ("http://127.0.0.1:", "its port is not a number"),
        ("http://127.0.0.1:99999", "its port is not a number"),
        (
            "http://user@127.0.0.1:11434",
            "it carries userinfo, which a local endpoint does not need",
        ),
    ] {
        match LoopbackUrl::parse(raw) {
            Err(LoopbackError::Malformed {
                url, reason: got, ..
            }) => {
                assert_eq!(url, raw);
                assert_eq!(got, reason, "{raw}");
            }
            other => panic!("{raw}: expected Malformed, got {other:?}"),
        }
    }
}

/// The checked URL is normalized: lowercased scheme, explicit port, no
/// trailing slash, brackets back on an IPv6 literal, and the trailing dot of
/// a fully qualified `localhost.` gone. What a caller appends a path to is
/// this, and nothing the person typed.
#[test]
fn an_accepted_url_is_normalized() {
    for (raw, base, scheme, path, host, port) in [
        (
            "http://127.0.0.1:11434",
            "http://127.0.0.1:11434",
            "http",
            "",
            "127.0.0.1",
            11434,
        ),
        (
            "  HTTP://127.0.0.1:11434//  ",
            "http://127.0.0.1:11434",
            "http",
            "",
            "127.0.0.1",
            11434,
        ),
        (
            "https://localhost",
            "https://localhost:443",
            "https",
            "",
            "localhost",
            443,
        ),
        (
            "http://localhost.:8000/v1/",
            "http://localhost:8000/v1",
            "http",
            "/v1",
            "localhost.",
            8000,
        ),
        (
            "http://[::1]/proxy?x=1",
            "http://[::1]:80/proxy?x=1",
            "http",
            "/proxy?x=1",
            "::1",
            80,
        ),
    ] {
        let url = LoopbackUrl::parse(raw).unwrap_or_else(|e| panic!("{raw}: {e}"));
        assert_eq!(url.as_str(), base, "{raw}");
        assert_eq!(url.to_string(), base, "{raw}");
        assert_eq!(url.scheme(), scheme, "{raw}");
        assert_eq!(url.path(), path, "{raw}");
        assert_eq!(url.host(), host, "{raw}");
        assert_eq!(url.port(), port, "{raw}");
    }
}

/// The resolver is the last gate, and it is the one that holds when
/// something inside the HTTP client decides to connect somewhere the caller
/// did not name. It answers for the addresses the guard validated and for
/// nothing else, so a proxy, a redirect, or a middleware asking for
/// `api.openai.com:443` gets an error instead of a socket. The same cases
/// are asserted through each caller's own alias in its `tests/no_remote.rs`.
#[test]
fn the_resolver_answers_only_for_the_validated_host_and_port() {
    let url = LoopbackUrl::parse("http://127.0.0.1:11434").unwrap();
    let resolver = LoopbackResolver::for_url(&url);

    let allowed = ureq::Resolver::resolve(&resolver, "127.0.0.1:11434")
        .expect("the validated address must resolve");
    assert_eq!(allowed, url.addrs());

    for netloc in [
        "api.openai.com:443",
        "openrouter.ai:443",
        "dashscope.aliyuncs.com:443",
        "8.8.8.8:11434",
        // Loopback, but not the port that was validated. A second local
        // service is still not the one the user pointed at.
        "127.0.0.1:9999",
        // The same machine under another name is still another name, and
        // the resolver does no lookup to find out otherwise.
        "localhost:11434",
        "[::1]:11434",
        "127.0.0.1",
        "",
    ] {
        let err = ureq::Resolver::resolve(&resolver, netloc)
            .expect_err("a netloc the guard never validated resolved");
        assert_eq!(err.kind(), std::io::ErrorKind::PermissionDenied, "{netloc}");
    }
}

/// A name is matched without case and an IPv6 literal with or without its
/// brackets, because that is how ureq hands them over. Neither widens what
/// the resolver answers with: it is still the validated addresses.
#[test]
fn the_resolver_matches_the_validated_name_as_ureq_writes_it() {
    let url = LoopbackUrl::parse("http://LocalHost:11434").unwrap();
    let resolver = LoopbackResolver::for_url(&url);
    for netloc in ["localhost:11434", "LOCALHOST:11434"] {
        assert_eq!(
            ureq::Resolver::resolve(&resolver, netloc).unwrap(),
            url.addrs()
        );
    }

    let url = LoopbackUrl::parse("http://[::1]:11434").unwrap();
    let resolver = LoopbackResolver::for_url(&url);
    for netloc in ["[::1]:11434", "::1:11434"] {
        assert_eq!(
            ureq::Resolver::resolve(&resolver, netloc).unwrap(),
            url.addrs()
        );
    }
}

/// The caller's phrases go in the caller's slots and nowhere else. The
/// sentence around them is this crate's, so two callers cannot drift into
/// saying different things about the same refusal.
#[test]
fn a_refusal_is_written_in_the_callers_phrases() {
    let message = |raw: &str| LoopbackUrl::parse(raw).unwrap_err().to_string();
    assert_eq!(
        message("http://[::1"),
        r#""http://[::1" is not a usable test endpoint: its bracket is unclosed"#
    );
    assert_eq!(
        message("ftp://127.0.0.1/"),
        r#"the "ftp" scheme is not a test endpoint; use http or https on loopback"#
    );
    assert_eq!(
        message("http://8.8.8.8:1"),
        "refusing to send test bytes to 8.8.8.8: that is not a loopback address. \
         Test bytes stay here"
    );

    let url = LoopbackUrl::parse("http://127.0.0.1:11434").unwrap();
    let resolver = LoopbackResolver::for_url(&url);
    let err = ureq::Resolver::resolve(&resolver, "evil.example:443").unwrap_err();
    assert_eq!(
        err.to_string(),
        "refusing to connect to evil.example:443: tests are pinned to 127.0.0.1:11434, \
         and nowhere else"
    );
}
