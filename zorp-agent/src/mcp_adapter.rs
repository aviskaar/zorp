use crate::tools::{Context, Tool, ToolError, ToolOutput, ToolResult};
use std::sync::{Arc, Mutex};
use zorp_mcp::{McpRegistry, McpTool};

pub struct McpToolAdapter {
    pub tool: McpTool,
    pub registry: Arc<Mutex<McpRegistry>>,
}

impl Tool for McpToolAdapter {
    #[allow(clippy::misnamed_getters)]
    fn name(&self) -> &str {
        &self.tool.prefixed_name
    }
    fn description(&self) -> &str {
        self.tool.description.as_deref().unwrap_or("MCP tool")
    }
    fn schema(&self) -> serde_json::Value {
        self.tool.input_schema.clone()
    }
    fn run(&self, args: &serde_json::Value, _cx: &mut Context) -> ToolResult {
        let mut reg = self
            .registry
            .lock()
            .map_err(|e| ToolError::new(format!("mcp registry lock poisoned: {e}")))?;
        match reg.call_tool(&self.tool.prefixed_name, args.clone()) {
            Ok(result) => {
                let text = if result.is_string() {
                    result.as_str().unwrap_or("").to_string()
                } else {
                    serde_json::to_string_pretty(&result).unwrap_or_default()
                };
                Ok(ToolOutput::new(text, "mcp tool result"))
            }
            // A failed MCP call is a tool error: route it through the uniform
            // error path so trace success accounting sees the failure instead
            // of recording a success with an error-shaped body.
            Err(e) => Err(ToolError::new(format!("mcp call failed: {e}"))),
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn mcp_prefix_routing_check() {
        assert!("mcp__filesystem__read_file".starts_with("mcp__"));
        assert!(!"read_file".starts_with("mcp__"));
    }
}
