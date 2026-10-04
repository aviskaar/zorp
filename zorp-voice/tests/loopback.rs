//! This crate's side of the loopback guard: how a refusal reads, which
//! endpoints voice can start a runtime on, and the default endpoint. The
//! guard itself lives in `zorp-loopback` and is tested there, against the
//! one copy. What this crate still owns is the wording, because only this
//! crate knows the thing being refused is a recording of somebody's voice.
//! Every sentence is pinned word for word, because a change here is a
//! change to what a person sees when their endpoint is refused.

use zorp_voice::{LoopbackResolver, LoopbackUrl};

fn refusal(raw: &str) -> String {
    LoopbackUrl::parse(raw).unwrap_err().to_string()
}

#[test]
fn a_malformed_endpoint_reads_the_same() {
    assert_eq!(
        refusal(""),
        r#""" is not a usable voice endpoint: it is empty"#
    );
    assert_eq!(
        refusal("127.0.0.1:8000"),
        r#""127.0.0.1:8000" is not a usable voice endpoint: it has no scheme"#
    );
    assert_eq!(
        refusal("http://user@127.0.0.1:8000"),
        r#""http://user@127.0.0.1:8000" is not a usable voice endpoint: it carries userinfo, which a local endpoint does not need"#
    );
}

#[test]
fn a_wrong_scheme_reads_the_same() {
    assert_eq!(
        refusal("ftp://127.0.0.1/"),
        r#"the "ftp" scheme is not a voice endpoint; use http or https on loopback"#
    );
}

#[test]
fn an_off_device_endpoint_reads_the_same() {
    assert_eq!(
        refusal("http://8.8.8.8:8000"),
        "refusing to send recorded audio to 8.8.8.8: that is not a loopback address. \
         Voice is only ever sent to this machine"
    );
    assert_eq!(
        refusal("https://dashscope.aliyuncs.com/api/v1"),
        "refusing to send recorded audio to dashscope.aliyuncs.com: only a loopback address \
         or `localhost` is accepted. Voice is only ever sent to this machine"
    );
}

#[test]
fn a_resolver_refusal_reads_the_same() {
    let url = LoopbackUrl::parse("http://127.0.0.1:8000").unwrap();
    let resolver = LoopbackResolver::for_url(&url);
    let err = ureq::Resolver::resolve(&resolver, "api.openai.com:443").unwrap_err();
    assert_eq!(err.kind(), std::io::ErrorKind::PermissionDenied);
    assert_eq!(
        err.to_string(),
        "refusing to connect to api.openai.com:443: voice is pinned to 127.0.0.1:8000"
    );
}

/// Whether `qwen-asr-serve` can bind the URL itself is voice's question,
/// not the guard's: plain `http` and no path. Pinned here so moving the
/// guard cannot change which endpoints get automatic setup.
#[test]
fn direct_runtime_is_plain_http_with_no_path() {
    use zorp_voice::DirectRuntime;
    for (raw, direct) in [
        ("http://127.0.0.1:8000", true),
        ("http://127.0.0.1:8000/", true),
        ("HTTP://localhost:8000", true),
        ("http://[::1]:8000", true),
        ("https://127.0.0.1:8000", false),
        ("http://127.0.0.1:8000/proxy", false),
        ("http://127.0.0.1:8000?x=1", false),
        ("http://127.0.0.1:8000#frag", false),
    ] {
        let url = LoopbackUrl::parse(raw).unwrap();
        assert_eq!(url.supports_direct_runtime(), direct, "{raw}");
    }
}

/// The default when nothing is configured is `qwen-asr-serve` on loopback, and it
/// passes its own guard. A default that the guard refuses would mean the
/// feature is off for everyone until they set a variable.
#[test]
fn the_default_endpoint_is_on_device() {
    assert!(LoopbackUrl::parse(zorp_voice::DEFAULT_VOICE_URL).is_ok());
}
