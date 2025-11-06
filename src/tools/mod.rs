use anyhow::{Context, Result, anyhow};
use serde::Deserialize;

pub mod execute_command;
pub mod read_file;
pub mod write_file;

#[derive(Debug, Deserialize)]
pub struct ToolEnvelope {
    pub tool: String,
    pub arguments: serde_json::Value,
}

#[derive(Debug)]
pub enum ToolCall {
    ReadFile(read_file::Args),
    WriteFile(write_file::Args),
    ExecuteCommand(execute_command::Args),
}

/// Try to parse a tool call JSON from assistant content.
pub fn parse_tool_call(json_text: &str) -> Result<ToolCall> {
    let env: ToolEnvelope = serde_json::from_str(json_text)
        .with_context(|| "Assistant response was not a tool JSON object")?;
    match env.tool.as_str() {
        "read_file" => {
            let args: read_file::Args = serde_json::from_value(env.arguments)?;
            Ok(ToolCall::ReadFile(args))
        }
        "write_file" => {
            let args: write_file::Args = serde_json::from_value(env.arguments)?;
            Ok(ToolCall::WriteFile(args))
        }
        "execute_command" => {
            let args: execute_command::Args = serde_json::from_value(env.arguments)?;
            Ok(ToolCall::ExecuteCommand(args))
        }
        other => Err(anyhow!("Unknown tool: {}", other)),
    }
}
