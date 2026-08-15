//! Minimal stub MCP server over stdio, used only by
//! `zorp-agent/tests/validate_integration.rs`.
//!
//! Framing: plain newline-delimited JSON (verified against
//! `zorp-mcp/src/transport/stdio.rs`'s `StdioTransport`, which reads
//! one line at a time via `BufRead::read_line` and writes requests as
//! `serde_json::to_string(&req) + "\n"`). No Content-Length headers.
//! The real client (`McpServer::initialize` in `zorp-mcp/src/server.rs`)
//! does not actually send a `notifications/initialized` message despite
//! the spec allowing it, so this stub doesn't need to handle one either.
use serde_json::{json, Value};
use std::io::{self, BufRead, Write};

fn respond(id: &Value, result: Value) {
    let resp = json!({ "jsonrpc": "2.0", "id": id, "result": result });
    println!("{}", resp);
    io::stdout().flush().ok();
}

fn main() {
    let stdin = io::stdin();
    for line in stdin.lock().lines() {
        let line = match line {
            Ok(l) if !l.trim().is_empty() => l,
            _ => continue,
        };
        let req: Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(_) => continue,
        };
        let id = req.get("id").cloned().unwrap_or(Value::Null);
        match req.get("method").and_then(|m| m.as_str()) {
            Some("initialize") => respond(
                &id,
                json!({
                    "protocolVersion": "2024-11-05",
                    "capabilities": {},
                    "serverInfo": { "name": "stub-search", "version": "0.1.0" }
                }),
            ),
            Some("tools/list") => respond(
                &id,
                json!({
                    "tools": [{
                        "name": "search",
                        "description": "search the web",
                        "inputSchema": { "type": "object", "properties": { "query": { "type": "string" } } }
                    }]
                }),
            ),
            Some("tools/call") => respond(
                &id,
                json!({
                    "content": [{ "type": "text", "text": "Stub result: no prior work found on this exact question. A relevant benchmarking tool already exists in the target repo." }]
                }),
            ),
            _ => {}
        }
    }
}
