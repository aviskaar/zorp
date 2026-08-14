pub mod sse;
pub mod stdio;
pub mod streamable_http;

use crate::error::McpError;
use crate::protocol::{JsonRpcNotification, JsonRpcRequest, JsonRpcResponse};

pub trait Transport: Send {
    fn send(&mut self, req: JsonRpcRequest) -> Result<JsonRpcResponse, McpError>;
    /// Send a JSON-RPC notification: no id, no response expected.
    fn send_notification(&mut self, notif: JsonRpcNotification) -> Result<(), McpError>;
}
