mod common;

use common::{dead_port, Canary};
use zorp_voice::QwenAsr;

#[test]
fn a_proxy_in_the_environment_is_not_used() {
    let canary = Canary::new();
    std::env::set_var("HTTP_PROXY", &canary.base);
    std::env::set_var("http_proxy", &canary.base);
    std::env::set_var("ALL_PROXY", &canary.base);

    let client = QwenAsr::at(&dead_port(), "test-model").unwrap();
    let status = client.status();

    std::env::remove_var("HTTP_PROXY");
    std::env::remove_var("http_proxy");
    std::env::remove_var("ALL_PROXY");
    assert!(!status.runtime_reachable);
    assert_eq!(canary.hits(), 0, "the voice request went through the proxy");
}
