use crate::error::McpError;
use crate::protocol::{id_matches, JsonRpcNotification, JsonRpcRequest, JsonRpcResponse};
use crate::transport::Transport;
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::time::{Duration, Instant};

/// A stdio MCP server, read on a helper thread.
///
/// The thread exists so reads can time out. A pipe read cannot be given
/// a deadline the way `ureq` gives one to the HTTP transports, so
/// without it a server that accepts a request and never answers blocks
/// the whole CLI forever, with no output and no way out but Ctrl-C. A
/// server that ignores an unsupported method instead of returning
/// -32601 does exactly that.
pub struct StdioTransport {
    child: Child,
    stdin: Option<ChildStdin>,
    lines: Receiver<std::io::Result<String>>,
    timeout: Duration,
    server: String,
}

impl StdioTransport {
    pub fn spawn(
        server: &str,
        command: &str,
        args: &[String],
        env: &HashMap<String, String>,
        timeout_secs: u64,
    ) -> Result<Self, McpError> {
        let mut child = Command::new(command)
            .args(args)
            .envs(env)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| McpError::Connect(format!("failed to spawn '{command}': {e}")))?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| McpError::Connect("could not open stdin pipe".into()))?;
        let stdout_raw = child
            .stdout
            .take()
            .ok_or_else(|| McpError::Connect("could not open stdout pipe".into()))?;

        let (tx, lines) = mpsc::channel();
        std::thread::spawn(move || {
            let mut reader = BufReader::new(stdout_raw);
            loop {
                let mut line = String::new();
                match reader.read_line(&mut line) {
                    Ok(0) => break, // EOF: server closed stdout.
                    Ok(_) => {
                        if tx.send(Ok(line)).is_err() {
                            break; // Transport dropped; nobody is listening.
                        }
                    }
                    Err(e) => {
                        let _ = tx.send(Err(e));
                        break;
                    }
                }
            }
        });

        Ok(StdioTransport {
            child,
            stdin: Some(stdin),
            lines,
            timeout: Duration::from_secs(timeout_secs),
            server: server.to_string(),
        })
    }

    fn stdin(&mut self) -> Result<&mut ChildStdin, McpError> {
        self.stdin
            .as_mut()
            .ok_or_else(|| McpError::Transport("stdin already closed".into()))
    }

    pub fn encode_request(req: &JsonRpcRequest) -> String {
        let mut s = serde_json::to_string(req).expect("serialize JsonRpcRequest");
        s.push('\n');
        s
    }
}

impl Drop for StdioTransport {
    fn drop(&mut self) {
        // Close stdin first: a well-behaved server exits on EOF, which
        // lets it flush whatever it wants to. Then make sure it is gone,
        // so a wedged server (the case the read timeout exists for) does
        // not outlive the CLI as an orphan.
        self.stdin.take();
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Transport for StdioTransport {
    fn send(&mut self, req: JsonRpcRequest) -> Result<JsonRpcResponse, McpError> {
        let encoded = Self::encode_request(&req);
        let stdin = self.stdin()?;
        stdin
            .write_all(encoded.as_bytes())
            .map_err(|e| McpError::Transport(format!("stdin write: {e}")))?;
        stdin
            .flush()
            .map_err(|e| McpError::Transport(format!("stdin flush: {e}")))?;

        // The deadline covers the whole exchange, not each line. A
        // server that streams unrelated responses cannot extend it
        // indefinitely and starve this request.
        let target_id = req.id;
        let deadline = Instant::now() + self.timeout;
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            let line = match self.lines.recv_timeout(remaining) {
                Ok(Ok(line)) => line,
                Ok(Err(e)) => return Err(McpError::Transport(format!("stdout read: {e}"))),
                Err(RecvTimeoutError::Timeout) => {
                    return Err(McpError::Timeout {
                        server: self.server.clone(),
                        elapsed_secs: self.timeout.as_secs(),
                    })
                }
                Err(RecvTimeoutError::Disconnected) => {
                    return Err(McpError::Transport("server closed stdout".into()))
                }
            };
            let resp: JsonRpcResponse = serde_json::from_str(line.trim())
                .map_err(|e| McpError::Protocol(format!("bad JSON-RPC: {e}")))?;
            if id_matches(resp.id.as_ref(), &target_id) {
                return Ok(resp);
            }
        }
    }

    fn send_notification(&mut self, notif: JsonRpcNotification) -> Result<(), McpError> {
        let mut encoded = serde_json::to_string(&notif)
            .map_err(|e| McpError::Protocol(format!("serialize notification: {e}")))?;
        encoded.push('\n');
        let stdin = self.stdin()?;
        stdin
            .write_all(encoded.as_bytes())
            .map_err(|e| McpError::Transport(format!("stdin write: {e}")))?;
        stdin
            .flush()
            .map_err(|e| McpError::Transport(format!("stdin flush: {e}")))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::JsonRpcRequest;
    use std::collections::HashMap;

    #[test]
    fn encode_request_is_newline_terminated() {
        let req = JsonRpcRequest::new(42, "tools/list", None);
        let enc = StdioTransport::encode_request(&req);
        assert!(enc.ends_with('\n'));
        let _: serde_json::Value = serde_json::from_str(enc.trim_end()).unwrap();
    }

    #[test]
    fn spawn_invalid_command_returns_err() {
        let result = StdioTransport::spawn(
            "bad",
            "zorp_mcp_definitely_no_such_binary_xyz",
            &[],
            &HashMap::new(),
            5,
        );
        assert!(result.is_err());
    }

    /// A server that reads a request and never answers must not block
    /// the caller forever. `cat > /dev/null` swallows stdin and writes
    /// nothing, which is exactly the wedged-server shape.
    #[test]
    fn send_times_out_when_the_server_never_answers() {
        let mut t = StdioTransport::spawn(
            "quiet",
            "sh",
            &["-c".to_string(), "cat > /dev/null".to_string()],
            &HashMap::new(),
            1,
        )
        .expect("spawn sh");
        let started = std::time::Instant::now();
        let err = t
            .send(JsonRpcRequest::new(1, "tools/list", None))
            .expect_err("must not hang");
        assert!(
            matches!(err, McpError::Timeout { ref server, .. } if server == "quiet"),
            "expected a timeout naming the server, got {err:?}"
        );
        // Comfortably under the old behaviour, which was unbounded.
        assert!(started.elapsed() < Duration::from_secs(10));
    }

    /// A server that exits immediately is a closed pipe, not a timeout.
    #[test]
    fn send_reports_a_closed_pipe_rather_than_timing_out() {
        let mut t = StdioTransport::spawn("gone", "true", &[], &HashMap::new(), 30).expect("spawn");
        let err = t
            .send(JsonRpcRequest::new(1, "tools/list", None))
            .expect_err("no response is possible");
        assert!(
            !matches!(err, McpError::Timeout { .. }),
            "closed stdout should not wait for the full timeout, got {err:?}"
        );
    }
}
