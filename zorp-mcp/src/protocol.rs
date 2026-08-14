use serde::{Deserialize, Serialize};
use serde_json::Value;

/// The MCP protocol version this client speaks.
pub const PROTOCOL_VERSION: &str = "2024-11-05";

#[derive(Debug, Serialize)]
pub struct JsonRpcRequest {
    pub jsonrpc: &'static str,
    pub id: Value,
    pub method: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<Value>,
}
impl JsonRpcRequest {
    pub fn new(id: impl Into<Value>, method: impl Into<String>, params: Option<Value>) -> Self {
        JsonRpcRequest { jsonrpc: "2.0", id: id.into(), method: method.into(), params }
    }
}

#[derive(Debug, Deserialize)]
pub struct JsonRpcResponse {
    #[serde(default)]
    pub jsonrpc: String,
    pub id: Option<Value>,
    pub result: Option<Value>,
    pub error: Option<JsonRpcError>,
}

#[derive(Debug, Deserialize)]
pub struct JsonRpcError { pub code: i64, pub message: String }

#[derive(Debug, Serialize, Deserialize)]
pub struct JsonRpcNotification {
    #[serde(default)]
    pub jsonrpc: String,
    pub method: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<Value>,
}
impl JsonRpcNotification {
    pub fn new(method: impl Into<String>, params: Option<Value>) -> Self {
        JsonRpcNotification { jsonrpc: "2.0".into(), method: method.into(), params }
    }
}

/// True when a response id refers to the given request id.
/// The spec says servers echo the id unchanged, and it may be a number
/// or a string. Some servers echo a numeric id back as its string form,
/// so that coercion is accepted too.
pub fn id_matches(response_id: Option<&Value>, request_id: &Value) -> bool {
    let Some(rid) = response_id else { return false };
    if rid == request_id {
        return true;
    }
    match (rid, request_id) {
        (Value::String(s), Value::Number(n)) | (Value::Number(n), Value::String(s)) => {
            *s == n.to_string()
        }
        _ => false,
    }
}

#[derive(Debug, Clone)]
pub struct McpTool {
    pub server: String,
    pub name: String,
    pub prefixed_name: String,
    pub description: Option<String>,
    pub input_schema: Value,
}

pub fn mcp_prefix(server: &str, tool: &str) -> String {
    format!("mcp__{server}__{tool}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn request_serialises_correctly() {
        let req = JsonRpcRequest::new(1, "tools/list", None);
        let v = serde_json::to_value(&req).unwrap();
        assert_eq!(v["jsonrpc"], "2.0");
        assert_eq!(v["id"], 1);
        assert_eq!(v["method"], "tools/list");
    }
    #[test]
    fn response_result_parsed() {
        let raw = json!({"jsonrpc":"2.0","id":1,"result":{"tools":[]}});
        let resp: JsonRpcResponse = serde_json::from_value(raw).unwrap();
        assert!(resp.error.is_none());
        assert!(resp.result.is_some());
    }
    #[test]
    fn response_with_string_id_parses() {
        let raw = json!({"jsonrpc":"2.0","id":"abc-1","result":{}});
        let resp: JsonRpcResponse = serde_json::from_value(raw).unwrap();
        assert_eq!(resp.id, Some(json!("abc-1")));
    }
    #[test]
    fn response_without_jsonrpc_field_parses() {
        let raw = json!({"id":1,"result":{}});
        let resp: JsonRpcResponse = serde_json::from_value(raw).unwrap();
        assert_eq!(resp.jsonrpc, "");
        assert!(resp.result.is_some());
    }
    #[test]
    fn id_matches_exact_number_and_string() {
        assert!(id_matches(Some(&json!(7)), &json!(7)));
        assert!(id_matches(Some(&json!("x")), &json!("x")));
        assert!(!id_matches(Some(&json!(7)), &json!(8)));
        assert!(!id_matches(None, &json!(1)));
    }
    #[test]
    fn id_matches_string_echo_of_number() {
        // A request sent with id 3 matched against a server echoing "3".
        assert!(id_matches(Some(&json!("3")), &json!(3)));
        assert!(id_matches(Some(&json!(3)), &json!("3")));
        assert!(!id_matches(Some(&json!("3x")), &json!(3)));
    }
    #[test]
    fn notification_serialises_without_id() {
        let n = JsonRpcNotification::new("notifications/initialized", None);
        let v = serde_json::to_value(&n).unwrap();
        assert_eq!(v["jsonrpc"], "2.0");
        assert_eq!(v["method"], "notifications/initialized");
        assert!(v.get("id").is_none());
        assert!(v.get("params").is_none());
    }
    #[test]
    fn mcp_prefix_format() {
        assert_eq!(mcp_prefix("filesystem", "read_file"), "mcp__filesystem__read_file");
    }
    #[test]
    fn mcp_tool_prefixed_name_correct() {
        let t = McpTool {
            server: "fs".into(), name: "read_file".into(),
            prefixed_name: mcp_prefix("fs", "read_file"),
            description: None, input_schema: json!({}),
        };
        assert_eq!(t.prefixed_name, "mcp__fs__read_file");
    }
}
