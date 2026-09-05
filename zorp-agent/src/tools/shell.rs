use super::{Context, Tool, ToolError, ToolOutput, ToolResult};
use serde_json::{json, Value};

/// What the optional `description` argument on a shell call is for, said to
/// the model.
///
/// Display only. Neither `run` below reads it: the agent hands it to the
/// renderer, the browser draws it on the tool line as the model's own words,
/// and the CLI ignores it.
const DESCRIPTION_HINT: &str = "A short phrase saying what this command does, for the person watching, in the present participle: \"Listing files in web/src\", \"Running the test suite\", \"Converting the report to PDF\". At most 60 characters. Not the command itself.";

fn shell_schema() -> Value {
    json!({"type":"object","properties":{"command":{"type":"string","description":"Command passed to /bin/sh -c"},"description":{"type":"string","description":DESCRIPTION_HINT}},"required":["command"],"additionalProperties":false})
}

pub struct RunCommand;

impl Tool for RunCommand {
    fn name(&self) -> &str {
        "run_command"
    }

    fn description(&self) -> &str {
        "Run a shell command at the repository root with timeout, cancellation, bounded output, and approval."
    }

    fn schema(&self) -> Value {
        shell_schema()
    }

    fn run(&self, args: &Value, cx: &mut Context) -> ToolResult {
        let command = args
            .get("command")
            .and_then(Value::as_str)
            .filter(|s| !s.trim().is_empty())
            .ok_or_else(|| ToolError::new("run_command requires a non-empty string 'command'"))?;
        cx.run_command(command)
    }
}

pub struct StartBackgroundProcess;

impl Tool for StartBackgroundProcess {
    fn name(&self) -> &str {
        "start_background_process"
    }

    fn description(&self) -> &str {
        "Starts a detached background process and returns its PID."
    }

    fn schema(&self) -> Value {
        shell_schema()
    }

    fn run(&self, args: &Value, cx: &mut Context) -> ToolResult {
        let command = args
            .get("command")
            .and_then(Value::as_str)
            .filter(|s| !s.trim().is_empty())
            .ok_or_else(|| ToolError::new("requires a non-empty string 'command'"))?;
        let pid = cx.start_background_process(command)?;
        Ok(ToolOutput::new(
            format!("Started background process with PID {}", pid),
            format!("started PID {}", pid),
        ))
    }
}

pub struct KillBackgroundProcess;

impl Tool for KillBackgroundProcess {
    fn name(&self) -> &str {
        "kill_background_process"
    }

    fn description(&self) -> &str {
        "Kills a background process by its PID."
    }

    fn schema(&self) -> Value {
        json!({"type":"object","properties":{"pid":{"type":"integer","description":"The PID of the background process to kill"}},"required":["pid"],"additionalProperties":false})
    }

    fn run(&self, args: &Value, cx: &mut Context) -> ToolResult {
        let pid = args
            .get("pid")
            .and_then(|v| v.as_u64())
            .ok_or_else(|| ToolError::new("requires an integer 'pid'"))? as u32;
        cx.kill_background_process(pid)?;
        Ok(ToolOutput::new(
            format!("Killed background process {}", pid),
            format!("killed PID {}", pid),
        ))
    }
}

pub struct ListBackgroundProcesses;

impl Tool for ListBackgroundProcesses {
    fn name(&self) -> &str {
        "list_background_processes"
    }

    fn description(&self) -> &str {
        "Lists all running background processes."
    }

    fn schema(&self) -> Value {
        json!({"type":"object","properties":{},"required":[]})
    }

    fn run(&self, _args: &Value, cx: &mut Context) -> ToolResult {
        let list = cx.list_background_processes();
        Ok(ToolOutput::new(list, "listed processes"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sandbox::cancel_token;
    use serde_json::json;

    #[test]
    fn schema_requires_only_command() {
        let schema = RunCommand.schema();
        assert_eq!(schema["required"], json!(["command"]));
        assert!(schema["properties"].get("cwd").is_none());
    }

    /// Both shell tools take the description, and neither requires it: a
    /// model that never writes one is not refused.
    #[test]
    fn both_shell_tools_take_an_optional_description() {
        for schema in [RunCommand.schema(), StartBackgroundProcess.schema()] {
            assert_eq!(schema["properties"]["description"]["type"], json!("string"));
            assert_eq!(schema["required"], json!(["command"]));
            assert_eq!(schema["additionalProperties"], json!(false));
        }
    }

    /// The description is for the person watching and changes nothing about
    /// what runs or what comes back.
    #[test]
    fn a_description_changes_nothing_about_the_run() {
        let dir = tempfile::tempdir().unwrap();
        let mut cx = Context::new(dir.path().to_path_buf(), cancel_token());
        let plain = RunCommand
            .run(&json!({"command":"printf hello"}), &mut cx)
            .unwrap();
        let described = RunCommand
            .run(
                &json!({"command":"printf hello","description":"Printing a greeting"}),
                &mut cx,
            )
            .unwrap();
        assert_eq!(described.content, plain.content);
        assert_eq!(described.summary, plain.summary);
        assert!(!described.content.contains("greeting"));
    }

    #[test]
    fn runs_and_reports_exit_status() {
        let dir = tempfile::tempdir().unwrap();
        let mut cx = Context::new(dir.path().to_path_buf(), cancel_token());
        let out = RunCommand
            .run(&json!({"command":"printf hello"}), &mut cx)
            .unwrap();
        assert!(out.content.contains("exit_status: 0"));
        assert!(out.content.contains("hello"));
    }

    #[test]
    fn missing_command_is_a_tool_error() {
        let dir = tempfile::tempdir().unwrap();
        let mut cx = Context::new(dir.path().to_path_buf(), cancel_token());
        assert!(RunCommand.run(&json!({}), &mut cx).is_err());
    }
}
