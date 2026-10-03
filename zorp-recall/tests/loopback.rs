//! This crate's side of the loopback guard: how a refusal reads, and the
//! default endpoint. The guard itself lives in `zorp-loopback` and is
//! tested there, against the one copy. What this crate still owns is the
//! wording, because only this crate knows the thing being refused is
//! somebody's conversation text. Every sentence is pinned word for word,
//! because a change here is a change to what a person sees when their
//! endpoint is refused.

use zorp_recall::{LoopbackResolver, LoopbackUrl};

fn refusal(raw: &str) -> String {
    LoopbackUrl::parse(raw).unwrap_err().to_string()
}

#[test]
fn a_malformed_endpoint_reads_the_same() {
    assert_eq!(refusal(""), r#""" is not a usable endpoint: it is empty"#);
    assert_eq!(
        refusal("127.0.0.1:11434"),
        r#""127.0.0.1:11434" is not a usable endpoint: it has no scheme"#
    );
    assert_eq!(
        refusal("http://user@127.0.0.1:11434"),
        r#""http://user@127.0.0.1:11434" is not a usable endpoint: it carries userinfo, which a local endpoint does not need"#
    );
}

#[test]
fn a_wrong_scheme_reads_the_same() {
    assert_eq!(
        refusal("ftp://127.0.0.1/"),
        r#"the "ftp" scheme is not an embedding endpoint; use http or https on loopback"#
    );
}

#[test]
fn an_off_device_endpoint_reads_the_same() {
    assert_eq!(
        refusal("http://8.8.8.8:11434"),
        "refusing to embed conversations at 8.8.8.8: that is not a loopback address. \
         Conversation text is only ever sent to this machine"
    );
    assert_eq!(
        refusal("https://api.openai.com/v1"),
        "refusing to embed conversations at api.openai.com: only a loopback address or \
         `localhost` is accepted. Conversation text is only ever sent to this machine"
    );
}

#[test]
fn a_resolver_refusal_reads_the_same() {
    let url = LoopbackUrl::parse("http://127.0.0.1:11434").unwrap();
    let resolver = LoopbackResolver::for_url(&url);
    let err = ureq::Resolver::resolve(&resolver, "api.openai.com:443").unwrap_err();
    assert_eq!(err.kind(), std::io::ErrorKind::PermissionDenied);
    assert_eq!(
        err.to_string(),
        "refusing to connect to api.openai.com:443: this build only talks to 127.0.0.1:11434, \
         because conversation text never leaves this machine"
    );
}

/// The default when nothing is configured is Ollama on loopback, and it
/// passes its own guard. A default that the guard refuses would mean the
/// feature is off for everyone until they set a variable.
#[test]
fn the_default_endpoint_is_on_device() {
    assert!(LoopbackUrl::parse(zorp_recall::DEFAULT_EMBED_URL).is_ok());
}
