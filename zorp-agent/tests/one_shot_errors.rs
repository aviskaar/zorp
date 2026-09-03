use std::io::{Read, Write};
use std::net::TcpListener;
use std::process::Command;

fn drain_request(stream: &mut std::net::TcpStream) {
    let mut request = Vec::new();
    let mut buffer = [0u8; 1024];
    let header_end = loop {
        let read = stream.read(&mut buffer).unwrap();
        assert!(read > 0);
        request.extend_from_slice(&buffer[..read]);
        if let Some(end) = request.windows(4).position(|bytes| bytes == b"\r\n\r\n") {
            break end + 4;
        }
    };
    let length = String::from_utf8_lossy(&request[..header_end])
        .lines()
        .find_map(|line| line.strip_prefix("Content-Length: "))
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(0);
    while request.len() < header_end + length {
        let read = stream.read(&mut buffer).unwrap();
        assert!(read > 0);
        request.extend_from_slice(&buffer[..read]);
    }
}

#[test]
fn one_shot_reports_a_refused_second_stream_request() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    std::thread::spawn(move || {
        let (mut first, _) = listener.accept().unwrap();
        drain_request(&mut first);
        let tool_call = r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"id":"call_1","type":"function","function":{"name":"read_file","arguments":"{\"path\":\"small.txt\"}"}}]}}]}"#;
        write!(
            first,
            "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\ndata: {tool_call}\n\ndata: [DONE]\n\n"
        )
        .unwrap();
        first.flush().unwrap();
        // A real server closes after Connection: close. Without this the agent
        // waits for end of stream until the idle timeout.
        drop(first);

        let (mut second, _) = listener.accept().unwrap();
        drain_request(&mut second);
        let body = r#"{"error":{"message":"prefill rejected"}}"#;
        write!(
            second,
            "HTTP/1.1 413 Payload Too Large\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        )
        .unwrap();
        second.flush().unwrap();
    });

    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("small.txt"), "small\n").unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_zorp-agent"))
        .current_dir(dir.path())
        .args([
            "--yes",
            "--no-verify",
            "--base-url",
            &format!("http://{address}/v1"),
            "--model",
            "test-model",
            "read the file",
        ])
        .output()
        .unwrap();

    assert!(
        !output.status.success(),
        "the refused request exited successfully"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("zorp-agent: "), "stderr was: {stderr}");
    assert!(
        stderr.contains("prefill rejected") || stderr.contains("413"),
        "the provider refusal was not named: {stderr}"
    );
}
