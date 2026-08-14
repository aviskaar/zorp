use crate::config::{ServerConfig, TransportKind, TrustLevel};
use crate::error::McpError;
use crate::protocol::{
    mcp_prefix, JsonRpcNotification, JsonRpcRequest, JsonRpcResponse, McpTool, PROTOCOL_VERSION,
};
use crate::transport::{stdio::StdioTransport, Transport};
use serde_json::json;

pub struct McpServer {
    pub name: String,
    pub trust: TrustLevel,
    transport: Box<dyn Transport>,
    id_counter: u64,
}

impl McpServer {
    pub fn from_config(cfg: &ServerConfig) -> Result<Self, McpError> {
        let connect_timeout = cfg.timeout_secs.unwrap_or(10);
        let transport: Box<dyn Transport> = match &cfg.transport {
            TransportKind::Stdio => {
                let cmd = cfg.command.as_deref().ok_or_else(|| {
                    McpError::Config(format!("server '{}': stdio requires `command`", cfg.name))
                })?;
                Box::new(StdioTransport::spawn(
                    cmd,
                    &cfg.args,
                    &cfg.env,
                    connect_timeout,
                )?)
            }
            TransportKind::StreamableHttp => {
                let url = cfg.url.clone().ok_or_else(|| {
                    McpError::Config(format!(
                        "server '{}': streamable_http requires `url`",
                        cfg.name
                    ))
                })?;
                Box::new(
                    crate::transport::streamable_http::StreamableHttpTransport::new(
                        url,
                        cfg.headers.clone(),
                        cfg.timeout_secs.unwrap_or(30),
                    ),
                )
            }
            TransportKind::Sse => {
                let url = cfg.url.clone().ok_or_else(|| {
                    McpError::Config(format!("server '{}': sse requires `url`", cfg.name))
                })?;
                Box::new(crate::transport::sse::SseTransport::new(
                    url,
                    cfg.headers.clone(),
                    cfg.timeout_secs.unwrap_or(30),
                ))
            }
        };
        Ok(McpServer {
            name: cfg.name.clone(),
            trust: cfg.trust.clone(),
            transport,
            id_counter: 1,
        })
    }

    pub(crate) fn next_id(&mut self) -> u64 {
        let id = self.id_counter;
        self.id_counter += 1;
        id
    }

    pub(crate) fn send_request(
        &mut self,
        req: JsonRpcRequest,
    ) -> Result<JsonRpcResponse, McpError> {
        self.transport.send(req)
    }

    pub fn initialize(&mut self) -> Result<(), McpError> {
        let id = self.next_id();
        let req = JsonRpcRequest::new(
            id,
            "initialize",
            Some(json!({
                "protocolVersion": PROTOCOL_VERSION,
                "capabilities": {},
                "clientInfo": { "name": "zorp-mcp", "version": env!("CARGO_PKG_VERSION") }
            })),
        );
        let resp = self.transport.send(req)?;
        if let Some(err) = resp.error {
            return Err(McpError::ServerError {
                code: err.code,
                message: err.message,
            });
        }
        // The spec requires this notification after a successful initialize.
        // SDK-based servers gate tools/list on it.
        self.transport
            .send_notification(JsonRpcNotification::new("notifications/initialized", None))?;
        Ok(())
    }

    pub fn list_tools(&mut self) -> Result<Vec<McpTool>, McpError> {
        let mut tools = Vec::new();
        let mut cursor: Option<String> = None;
        loop {
            let id = self.next_id();
            let params = cursor.as_ref().map(|c| json!({"cursor": c}));
            let resp = self
                .transport
                .send(JsonRpcRequest::new(id, "tools/list", params))?;
            if let Some(err) = resp.error {
                return Err(McpError::ServerError {
                    code: err.code,
                    message: err.message,
                });
            }
            let result = resp
                .result
                .ok_or_else(|| McpError::Protocol("tools/list: missing result".into()))?;
            let arr = result["tools"].as_array().ok_or_else(|| {
                McpError::Protocol("tools/list: result.tools is not an array".into())
            })?;
            for t in arr {
                let name = t["name"]
                    .as_str()
                    .ok_or_else(|| McpError::Protocol("tool entry missing name".into()))?
                    .to_string();
                tools.push(McpTool {
                    server: self.name.clone(),
                    prefixed_name: mcp_prefix(&self.name, &name),
                    name,
                    description: t["description"].as_str().map(str::to_string),
                    input_schema: t.get("inputSchema").cloned().unwrap_or(json!({})),
                });
            }
            match result.get("nextCursor").and_then(|v| v.as_str()) {
                None => break,
                Some(next) => {
                    if cursor.as_deref() == Some(next) {
                        return Err(McpError::Protocol(format!(
                            "tools/list: server repeated cursor '{next}', aborting pagination"
                        )));
                    }
                    cursor = Some(next.to_string());
                }
            }
        }
        Ok(tools)
    }

    pub fn read_resource(&mut self, uri: &str) -> Result<String, McpError> {
        let id = self.next_id();
        let resp = self.send_request(JsonRpcRequest::new(
            id,
            "resources/read",
            Some(json!({"uri": uri})),
        ))?;
        if let Some(err) = resp.error {
            return Err(McpError::ServerError {
                code: err.code,
                message: err.message,
            });
        }
        let result = resp
            .result
            .ok_or_else(|| McpError::Protocol("resources/read: missing result".into()))?;
        Ok(result["contents"]
            .as_array()
            .and_then(|a| a.first())
            .and_then(|c| c["text"].as_str())
            .unwrap_or("")
            .to_string())
    }

    pub fn list_prompt_names(&mut self) -> Result<Vec<String>, McpError> {
        let id = self.next_id();
        let resp = self.send_request(JsonRpcRequest::new(id, "prompts/list", None))?;
        if let Some(err) = resp.error {
            return Err(McpError::ServerError {
                code: err.code,
                message: err.message,
            });
        }
        let result = resp
            .result
            .ok_or_else(|| McpError::Protocol("prompts/list: missing result".into()))?;
        Ok(result["prompts"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|p| p["name"].as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default())
    }

    pub fn get_prompt(&mut self, name: &str) -> Result<String, McpError> {
        let id = self.next_id();
        let resp = self.send_request(JsonRpcRequest::new(
            id,
            "prompts/get",
            Some(json!({"name": name})),
        ))?;
        if let Some(err) = resp.error {
            return Err(McpError::ServerError {
                code: err.code,
                message: err.message,
            });
        }
        let result = resp
            .result
            .ok_or_else(|| McpError::Protocol("prompts/get: missing result".into()))?;
        Ok(result["messages"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|m| m["content"]["text"].as_str())
                    .collect::<Vec<_>>()
                    .join("\n")
            })
            .unwrap_or_default())
    }

    pub fn sampling_create_message(
        &mut self,
        messages: serde_json::Value,
        model_prefs: serde_json::Value,
        base_url: &str,
        api_key: &str,
        model: &str,
    ) -> Result<serde_json::Value, McpError> {
        let body = json!({"model": model, "messages": messages, "max_tokens": model_prefs.get("maxTokens").and_then(|v| v.as_u64()).unwrap_or(1024)});
        let url = format!("{}/chat/completions", base_url.trim_end_matches('/'));
        let mut headers = std::collections::HashMap::new();
        if !api_key.is_empty() {
            headers.insert("Authorization".to_string(), format!("Bearer {api_key}"));
        }
        headers.insert("Content-Type".to_string(), "application/json".to_string());
        let header_refs: Vec<(&str, &str)> = headers
            .iter()
            .map(|(k, v)| (k.as_str(), v.as_str()))
            .collect();
        zorp::zorp_raw(&url, &header_refs, body)
            .map_err(|e| McpError::Transport(format!("sampling LLM call failed: {e}")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::mcp_prefix;
    use serde_json::{json, Value};
    use std::sync::{Arc, Mutex};

    #[test]
    fn prefix_formula() {
        assert_eq!(
            mcp_prefix("myserver", "do_thing"),
            "mcp__myserver__do_thing"
        );
    }

    #[test]
    fn initialize_request_has_correct_method() {
        let req = JsonRpcRequest::new(
            1,
            "initialize",
            Some(
                json!({"protocolVersion":PROTOCOL_VERSION,"capabilities":{},"clientInfo":{"name":"zorp-mcp","version":"0.1.0"}}),
            ),
        );
        let v = serde_json::to_value(&req).unwrap();
        assert_eq!(v["method"], "initialize");
    }

    /// Shared, inspectable state behind a mock transport.
    #[derive(Default)]
    struct MockState {
        /// Notification methods received, in order.
        notified: Vec<String>,
        /// Request (method, params) pairs received, in order.
        sent: Vec<(String, Option<Value>)>,
        /// When true, tools/list errors until notifications/initialized arrives.
        require_initialized: bool,
        /// tools/list results by page. A cursor param "N" selects page N,
        /// no cursor selects page 0.
        tool_pages: Vec<Value>,
    }

    struct MockTransport {
        state: Arc<Mutex<MockState>>,
    }

    fn ok_resp(id: Value, result: Value) -> JsonRpcResponse {
        serde_json::from_value(json!({"jsonrpc":"2.0","id":id,"result":result})).unwrap()
    }

    fn err_resp(id: Value, code: i64, message: &str) -> JsonRpcResponse {
        serde_json::from_value(
            json!({"jsonrpc":"2.0","id":id,"error":{"code":code,"message":message}}),
        )
        .unwrap()
    }

    impl Transport for MockTransport {
        fn send(&mut self, req: JsonRpcRequest) -> Result<JsonRpcResponse, McpError> {
            let mut st = self.state.lock().unwrap();
            st.sent.push((req.method.clone(), req.params.clone()));
            match req.method.as_str() {
                "initialize" => Ok(ok_resp(
                    req.id,
                    json!({
                        "protocolVersion": PROTOCOL_VERSION,
                        "capabilities": {},
                        "serverInfo": {"name": "mock", "version": "0"}
                    }),
                )),
                "tools/list" => {
                    if st.require_initialized
                        && !st.notified.iter().any(|m| m == "notifications/initialized")
                    {
                        return Ok(err_resp(req.id, -32002, "server not initialized"));
                    }
                    let page: usize = req
                        .params
                        .as_ref()
                        .and_then(|p| p["cursor"].as_str())
                        .and_then(|c| c.parse().ok())
                        .unwrap_or(0);
                    let idx = page.min(st.tool_pages.len().saturating_sub(1));
                    Ok(ok_resp(req.id, st.tool_pages[idx].clone()))
                }
                other => Ok(err_resp(
                    req.id,
                    -32601,
                    &format!("method not found: {other}"),
                )),
            }
        }

        fn send_notification(&mut self, notif: JsonRpcNotification) -> Result<(), McpError> {
            self.state.lock().unwrap().notified.push(notif.method);
            Ok(())
        }
    }

    fn mock_server(state: Arc<Mutex<MockState>>) -> McpServer {
        McpServer {
            name: "mock".into(),
            trust: TrustLevel::Sandbox,
            transport: Box::new(MockTransport { state }),
            id_counter: 1,
        }
    }

    #[test]
    fn initialize_sends_initialized_notification() {
        let state = Arc::new(Mutex::new(MockState {
            tool_pages: vec![json!({"tools": []})],
            ..Default::default()
        }));
        let mut srv = mock_server(state.clone());
        srv.initialize().unwrap();
        let st = state.lock().unwrap();
        assert_eq!(st.notified, vec!["notifications/initialized".to_string()]);
    }

    #[test]
    fn tools_list_works_on_server_that_gates_on_initialized() {
        // The mock rejects tools/list until it has seen the notification,
        // like SDK-based servers do.
        let state = Arc::new(Mutex::new(MockState {
            require_initialized: true,
            tool_pages: vec![json!({"tools": [{"name": "gated_tool"}]})],
            ..Default::default()
        }));
        let mut srv = mock_server(state.clone());
        srv.initialize().unwrap();
        let tools = srv.list_tools().unwrap();
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].prefixed_name, "mcp__mock__gated_tool");
    }

    #[test]
    fn tools_list_follows_next_cursor_across_pages() {
        let state = Arc::new(Mutex::new(MockState {
            tool_pages: vec![
                json!({"tools": [{"name": "a"}, {"name": "b"}], "nextCursor": "1"}),
                json!({"tools": [{"name": "c"}]}),
            ],
            ..Default::default()
        }));
        let mut srv = mock_server(state.clone());
        let tools = srv.list_tools().unwrap();
        let names: Vec<&str> = tools.iter().map(|t| t.name.as_str()).collect();
        assert_eq!(names, vec!["a", "b", "c"]);
        let st = state.lock().unwrap();
        let list_calls: Vec<&Option<Value>> = st
            .sent
            .iter()
            .filter(|(m, _)| m == "tools/list")
            .map(|(_, p)| p)
            .collect();
        assert_eq!(list_calls.len(), 2);
        assert!(
            list_calls[0].is_none(),
            "first page should not send a cursor"
        );
        assert_eq!(list_calls[1].as_ref().unwrap()["cursor"], "1");
    }

    #[test]
    fn tools_list_repeated_cursor_errors_instead_of_looping() {
        // The mock always returns nextCursor "1", including on page 1 itself,
        // so a naive client would loop forever.
        let state = Arc::new(Mutex::new(MockState {
            tool_pages: vec![
                json!({"tools": [{"name": "a"}], "nextCursor": "1"}),
                json!({"tools": [{"name": "b"}], "nextCursor": "1"}),
            ],
            ..Default::default()
        }));
        let mut srv = mock_server(state);
        let err = srv.list_tools().unwrap_err();
        assert!(
            err.to_string().contains("repeated cursor"),
            "unexpected error: {err}"
        );
    }
}
