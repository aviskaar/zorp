use tempfile::tempdir;
use zorp_web::{find_ui, serve, ServeOptions};

#[tokio::test]
async fn serve_binds_ephemeral_port() {
    let options = ServeOptions {
        bind: "127.0.0.1".to_string(),
        port: 0,
        token: None,
        ui_dir: None,
        workspace: None,
        allow_origin: Vec::new(),
        additional_ui_candidates: Vec::new(),
    };

    let running = serve(options).await.expect("serve should bind port 0");
    assert_ne!(running.addr.port(), 0);
    assert_eq!(running.addr.ip(), std::net::Ipv4Addr::new(127, 0, 0, 1));
    running.handle.abort();
}

#[test]
fn find_ui_checks_additional_candidates_first() {
    let dir = tempdir().unwrap();
    let custom_ui = dir.path().join("bundle/web");
    std::fs::create_dir_all(&custom_ui).unwrap();
    std::fs::write(custom_ui.join("index.html"), "<html></html>").unwrap();

    let found = find_ui(None, std::slice::from_ref(&custom_ui));
    assert_eq!(found, Some(custom_ui));
}
