use crate::error::McpError;
use crate::protocol::{
    id_matches, JsonRpcNotification, JsonRpcRequest, JsonRpcResponse, PROTOCOL_VERSION,
};
use crate::transport::Transport;
use std::collections::HashMap;

pub struct StreamableHttpTransport {
    url: String,
    pub(crate) resolved_headers: HashMap<String, String>,
    agent: ureq::Agent,
    /// Session id issued by the server on the initialize response.
    /// Sent back on every later request and notification.
    session_id: Option<String>,
}

impl StreamableHttpTransport {
    pub fn new(url: String, headers: HashMap<String, String>, call_timeout_secs: u64) -> Self {
        let resolved_headers = headers
            .into_iter()
            .map(|(k, v)| (k, expand_env(&v, |k| std::env::var(k).ok())))
            .collect();
        let agent = ureq::AgentBuilder::new()
            .timeout(std::time::Duration::from_secs(call_timeout_secs))
            .build();
        StreamableHttpTransport {
            url,
            resolved_headers,
            agent,
            session_id: None,
        }
    }

    fn build_post(&self) -> ureq::Request {
        let mut request = self
            .agent
            .post(&self.url)
            .set("Content-Type", "application/json")
            .set("Accept", "application/json, text/event-stream")
            .set("MCP-Protocol-Version", PROTOCOL_VERSION);
        if let Some(sid) = &self.session_id {
            request = request.set("Mcp-Session-Id", sid);
        }
        for (k, v) in &self.resolved_headers {
            request = request.set(k, v);
        }
        request
    }

    fn capture_session_id(&mut self, response: &ureq::Response) {
        if let Some(sid) = response.header("mcp-session-id") {
            self.session_id = Some(sid.to_string());
        }
    }
}

fn expand_env<F>(s: &str, mut get_env: F) -> String
where
    F: FnMut(&str) -> Option<String>,
{
    let mut result = String::new();
    let mut remainder = s;
    while let Some(idx) = remainder.find('$') {
        result.push_str(&remainder[..idx]);
        let rest = &remainder[idx + 1..];
        let end = rest
            .find(|c: char| !c.is_alphanumeric() && c != '_')
            .unwrap_or(rest.len());
        let var_name = &rest[..end];
        if !var_name.is_empty() {
            if let Some(val) = get_env(var_name) {
                result.push_str(&val);
            } else {
                result.push('$');
                result.push_str(var_name);
            }
        } else {
            result.push('$');
        }
        remainder = &rest[end..];
    }
    result.push_str(remainder);
    result
}

impl Transport for StreamableHttpTransport {
    fn send(&mut self, req: JsonRpcRequest) -> Result<JsonRpcResponse, McpError> {
        let body = serde_json::to_string(&req)
            .map_err(|e| McpError::Protocol(format!("serialize: {e}")))?;
        let response = self
            .build_post()
            .send_string(&body)
            .map_err(|e| McpError::Transport(format!("HTTP POST failed: {e}")))?;
        self.capture_session_id(&response);
        let ct = response
            .header("content-type")
            .unwrap_or("application/json")
            .to_string();
        if ct.contains("text/event-stream") {
            parse_sse_response(response, &req.id)
        } else {
            let text = response
                .into_string()
                .map_err(|e| McpError::Transport(format!("read body: {e}")))?;
            serde_json::from_str(&text)
                .map_err(|e| McpError::Protocol(format!("bad JSON-RPC: {e}")))
        }
    }

    fn send_notification(&mut self, notif: JsonRpcNotification) -> Result<(), McpError> {
        let body = serde_json::to_string(&notif)
            .map_err(|e| McpError::Protocol(format!("serialize notification: {e}")))?;
        // A notification is a plain POST. The server replies 202 (or 200)
        // and the body carries nothing we need.
        let response = self
            .build_post()
            .send_string(&body)
            .map_err(|e| McpError::Transport(format!("HTTP POST failed: {e}")))?;
        self.capture_session_id(&response);
        Ok(())
    }
}

fn parse_sse_response(
    response: ureq::Response,
    target_id: &serde_json::Value,
) -> Result<JsonRpcResponse, McpError> {
    use std::io::BufRead;
    let reader = std::io::BufReader::new(response.into_reader());
    let mut data_buf = String::new();
    for line in reader.lines() {
        let line = line.map_err(|e| McpError::Transport(format!("SSE read: {e}")))?;
        if let Some(data) = line.strip_prefix("data:") {
            let data = data.strip_prefix(' ').unwrap_or(data);
            data_buf.push_str(data);
            data_buf.push('\n');
        } else if line.is_empty() && !data_buf.is_empty() {
            if let Ok(resp) = serde_json::from_str::<JsonRpcResponse>(&data_buf) {
                if id_matches(resp.id.as_ref(), target_id) {
                    return Ok(resp);
                }
            }
            data_buf.clear();
        }
    }
    Err(McpError::Transport(format!(
        "SSE stream ended without response for id {target_id}"
    )))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::io::{Read as _, Write as _};
    use std::net::TcpListener;

    #[test]
    fn constructor_does_not_panic() {
        let _t = StreamableHttpTransport::new("https://example.com/mcp".into(), HashMap::new(), 30);
    }

    #[test]
    fn env_var_substitution_in_header_value() {
        let resolved = expand_env("Bearer $ZORP_TEST_TOKEN_XYZ", |k| {
            if k == "ZORP_TEST_TOKEN_XYZ" {
                Some("tok123".into())
            } else {
                None
            }
        });
        assert_eq!(resolved, "Bearer tok123");
    }

    /// Read one HTTP request off a stream: headers, then the
    /// Content-Length body if present.
    fn read_http_request(stream: &mut std::net::TcpStream) -> String {
        let mut buf = Vec::new();
        let mut chunk = [0u8; 1024];
        loop {
            let n = stream.read(&mut chunk).unwrap();
            if n == 0 {
                break;
            }
            buf.extend_from_slice(&chunk[..n]);
            let text = String::from_utf8_lossy(&buf);
            if let Some(header_end) = text.find("\r\n\r\n") {
                let content_length = text
                    .lines()
                    .find_map(|l| {
                        let (k, v) = l.split_once(':')?;
                        if k.eq_ignore_ascii_case("content-length") {
                            v.trim().parse::<usize>().ok()
                        } else {
                            None
                        }
                    })
                    .unwrap_or(0);
                if buf.len() >= header_end + 4 + content_length {
                    break;
                }
            }
        }
        String::from_utf8_lossy(&buf).into_owned()
    }

    fn write_http_response(
        stream: &mut std::net::TcpStream,
        status: &str,
        extra_headers: &str,
        body: &str,
    ) {
        let resp = format!(
            "HTTP/1.1 {status}\r\nContent-Type: application/json\r\n{extra_headers}Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        stream.write_all(resp.as_bytes()).unwrap();
        stream.flush().unwrap();
    }

    #[test]
    fn session_id_captured_and_replayed_with_protocol_version_header() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let handle = std::thread::spawn(move || {
            let mut captured = Vec::new();
            // Request 1: initialize, respond with a session id.
            let (mut s, _) = listener.accept().unwrap();
            captured.push(read_http_request(&mut s));
            write_http_response(
                &mut s,
                "200 OK",
                "Mcp-Session-Id: sess-123\r\n",
                r#"{"jsonrpc":"2.0","id":1,"result":{}}"#,
            );
            // Request 2: a normal request, must carry the session id.
            let (mut s, _) = listener.accept().unwrap();
            captured.push(read_http_request(&mut s));
            write_http_response(
                &mut s,
                "200 OK",
                "",
                r#"{"jsonrpc":"2.0","id":2,"result":{"tools":[]}}"#,
            );
            // Request 3: a notification, must also carry the session id.
            let (mut s, _) = listener.accept().unwrap();
            captured.push(read_http_request(&mut s));
            write_http_response(&mut s, "202 Accepted", "", "");
            captured
        });

        let mut t = StreamableHttpTransport::new(format!("http://{addr}/mcp"), HashMap::new(), 5);
        let r1 = t.send(JsonRpcRequest::new(1, "initialize", None)).unwrap();
        assert!(r1.result.is_some());
        let r2 = t.send(JsonRpcRequest::new(2, "tools/list", None)).unwrap();
        assert!(r2.result.is_some());
        t.send_notification(JsonRpcNotification::new("notifications/initialized", None))
            .unwrap();

        let captured = handle.join().unwrap();
        let lower: Vec<String> = captured.iter().map(|c| c.to_lowercase()).collect();
        // No session id yet on the first request; protocol version always sent.
        assert!(
            !lower[0].contains("mcp-session-id"),
            "first request should have no session id"
        );
        assert!(lower[0].contains(&format!("mcp-protocol-version: {PROTOCOL_VERSION}")));
        // The captured session id rides on every later request.
        assert!(
            lower[1].contains("mcp-session-id: sess-123"),
            "second request missing session id: {}",
            captured[1]
        );
        assert!(lower[1].contains(&format!("mcp-protocol-version: {PROTOCOL_VERSION}")));
        assert!(
            lower[2].contains("mcp-session-id: sess-123"),
            "notification missing session id: {}",
            captured[2]
        );
        assert!(captured[2].contains("notifications/initialized"));
    }

    #[test]
    fn string_id_response_accepted_over_http() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let handle = std::thread::spawn(move || {
            let (mut s, _) = listener.accept().unwrap();
            let _req = read_http_request(&mut s);
            // Server echoes the numeric id as a string.
            write_http_response(
                &mut s,
                "200 OK",
                "",
                r#"{"jsonrpc":"2.0","id":"1","result":{"ok":true}}"#,
            );
        });
        let mut t = StreamableHttpTransport::new(format!("http://{addr}/mcp"), HashMap::new(), 5);
        let resp = t.send(JsonRpcRequest::new(1, "initialize", None)).unwrap();
        assert_eq!(resp.id, Some(serde_json::json!("1")));
        assert!(resp.result.is_some());
        handle.join().unwrap();
    }
}
