//! A proxy in the environment does not get the conversation text.
//!
//! Its own test binary because it sets a process-wide environment variable,
//! and tests inside one binary share a process. One test here, so nothing
//! else can see the variable while it is set.
//!
//! This is not a hypothetical. `ureq::AgentBuilder::new` turns on
//! `try_proxy_from_env` when the `proxy-from-env` feature is enabled
//! anywhere in the dependency graph, and feature unification means another
//! crate can enable it without this one asking. `HTTP_PROXY` is then
//! whatever the machine's environment says, which on a managed laptop is a
//! server belonging to somebody else.

mod common;

use common::Canary;
use zorp_recall::{Embedder, LoopbackUrl, OllamaEmbedder};

#[test]
fn a_proxy_in_the_environment_is_not_used() {
    let canary = Canary::new();
    let proxy = canary.base.clone();
    std::env::set_var("HTTP_PROXY", &proxy);
    std::env::set_var("http_proxy", &proxy);
    std::env::set_var("ALL_PROXY", &proxy);

    // A loopback port with nothing on it. The request must fail there, at
    // the address it was told to use, and not succeed via the proxy.
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    drop(listener);

    let url = LoopbackUrl::parse(&format!("http://{addr}")).unwrap();
    let err = OllamaEmbedder::new(url, "test-model")
        .embed("a private conversation about my medical history")
        .expect_err("nothing is listening, so this cannot succeed");

    std::env::remove_var("HTTP_PROXY");
    std::env::remove_var("http_proxy");
    std::env::remove_var("ALL_PROXY");

    assert_eq!(canary.hits(), 0, "the request went through the proxy");
    assert!(
        err.to_string().contains("no local embedder"),
        "unexpected failure: {err}"
    );
}
