use std::net::SocketAddr;

/// Port 0 so parallel test runs never collide on a fixed port.
async fn spawn() -> SocketAddr {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, zorp_web::api::router())
            .await
            .unwrap();
    });
    addr
}

async fn get(url: String) -> String {
    tokio::task::spawn_blocking(move || ureq::get(&url).call().unwrap().into_string().unwrap())
        .await
        .unwrap()
}

#[tokio::test]
async fn health_reports_ok() {
    let addr = spawn().await;
    let body = get(format!("http://{addr}/api/health")).await;
    assert!(body.contains("\"status\":\"ok\""), "got {body}");
}
