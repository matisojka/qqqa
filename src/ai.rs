use crate::config::{CliEngine, ResolvedTlsConfig};
use anyhow::{Context, Result, anyhow};
use bytes::Bytes;
use fs_err as fs;
use futures_util::StreamExt;
use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use reqwest::{Certificate, Client, RequestBuilder};
use rustls_pemfile::certs;
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::io::Cursor;
use std::path::Path;
use std::time::Duration;

const DEFAULT_MAX_COMPLETION_TOKENS: u32 = 800;
const DEFAULT_REQUEST_TIMEOUT_SECS: u64 = 180;
const DEFAULT_CONNECT_TIMEOUT_SECS: u64 = 10;
const DEFAULT_TEMPERATURE: f32 = 0.15;

/// Minimal OpenAI-compatible chat streaming delta payload
#[derive(Debug, Deserialize)]
struct ChatStreamChunkChoiceDelta {
    content: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ChatStreamChunkChoice {
    delta: Option<ChatStreamChunkChoiceDelta>,
    #[allow(dead_code)]
    index: Option<u32>,
}

#[derive(Debug, Deserialize)]
struct ChatStreamChunk {
    choices: Vec<ChatStreamChunkChoice>,
}

#[derive(Debug, Deserialize)]
struct ChatMessage {
    content: String,
}

#[derive(Debug, Deserialize)]
struct ChatChoice {
    message: ChatMessage,
}

#[derive(Debug, Deserialize)]
struct ChatResponse {
    choices: Vec<ChatChoice>,
}

// Types for tool-aware chat responses
#[derive(Debug, Deserialize)]
struct ToolFunctionSpecResp {
    name: String,
    // OpenAI-compatible APIs return arguments as a JSON string
    arguments: String,
}

#[derive(Debug, Deserialize)]
struct ToolCallResp {
    #[serde(rename = "type")]
    _type: String,
    function: ToolFunctionSpecResp,
}

#[derive(Debug, Deserialize)]
struct ChatMessageWithTools {
    #[serde(default)]
    content: Option<String>,
    #[allow(dead_code)]
    #[serde(default)]
    tool_calls: Option<Vec<ToolCallResp>>,
}

#[derive(Debug, Deserialize)]
struct ChatChoiceWithTools {
    message: ChatMessageWithTools,
}

#[derive(Debug, Deserialize)]
struct ChatResponseWithTools {
    choices: Vec<ChatChoiceWithTools>,
}

/// A simplified representation of the assistant's first choice.
pub enum AssistantReply {
    Content(String),
    ToolCall {
        name: String,
        arguments_json: String,
    },
}

#[derive(Clone, Copy)]
enum TemperatureDirective {
    Omit,
    Send(f32),
}

pub struct ChatClient {
    client: Client,
    base_url: String,
    api_key: String,
    reasoning_effort: Option<String>,
    temperature_override: Option<f32>,
    temperature_user_override: bool,
    default_headers: HeaderMap,
}

impl ChatClient {
    pub fn new(
        base_url: String,
        api_key: String,
        headers: HashMap<String, String>,
        tls: Option<&ResolvedTlsConfig>,
        request_timeout_secs: Option<u64>,
    ) -> Result<Self> {
        // Use rustls for TLS; set useful timeouts for robustness.
        let timeout =
            Duration::from_secs(request_timeout_secs.unwrap_or(DEFAULT_REQUEST_TIMEOUT_SECS));
        let mut builder = Client::builder()
            .timeout(timeout)
            .connect_timeout(Duration::from_secs(DEFAULT_CONNECT_TIMEOUT_SECS));
        if let Some(tls_cfg) = tls {
            for cert in load_root_certificates(&tls_cfg.ca_bundle_path)? {
                builder = builder.add_root_certificate(cert);
            }
        }
        let client = builder.build()?;
        let mut default_headers = HeaderMap::new();
        for (name, value) in headers {
            let header_name = HeaderName::from_bytes(name.as_bytes())
                .with_context(|| format!("Invalid header name '{}': must be ASCII", name))?;
            let header_value = HeaderValue::from_str(&value)
                .with_context(|| format!("Invalid header value for '{}': {}", name, value))?;
            default_headers.insert(header_name, header_value);
        }
        Ok(Self {
            client,
            base_url,
            api_key,
            reasoning_effort: None,
            temperature_override: None,
            temperature_user_override: false,
            default_headers,
        })
    }

    pub fn with_reasoning_effort(mut self, reasoning_effort: Option<String>) -> Self {
        self.reasoning_effort = reasoning_effort;
        self
    }

    pub fn with_temperature(mut self, temperature: Option<f32>, user_provided: bool) -> Self {
        self.temperature_override = temperature;
        self.temperature_user_override = user_provided && temperature.is_some();
        self
    }

    fn request_builder(&self) -> RequestBuilder {
        let mut builder = self.client.post(self.chat_url());
        if !self.default_headers.is_empty() {
            builder = builder.headers(self.default_headers.clone());
        }
        builder.bearer_auth(&self.api_key)
    }

    fn is_new_style_model(model: &str) -> bool {
        let lower = model.to_ascii_lowercase();
        const PREFIXES: [&str; 3] = ["gpt-5", "o1", "o3"];
        PREFIXES.iter().any(|prefix| lower.starts_with(prefix))
    }

    fn is_gpt5_model(model: &str) -> bool {
        model.to_ascii_lowercase().starts_with("gpt-5")
    }

    fn max_tokens_param(model: &str) -> &'static str {
        if Self::is_new_style_model(model) {
            "max_completion_tokens"
        } else {
            "max_tokens"
        }
    }

    fn temperature_directive(&self, model: &str) -> TemperatureDirective {
        if Self::is_gpt5_model(model) {
            if self.temperature_override.is_some() {
                TemperatureDirective::Send(1.0)
            } else {
                TemperatureDirective::Omit
            }
        } else {
            TemperatureDirective::Send(self.temperature_override.unwrap_or(DEFAULT_TEMPERATURE))
        }
    }

    fn default_reasoning_effort(model: &str) -> Option<&'static str> {
        if Self::is_gpt5_model(model) {
            Some("minimal")
        } else {
            None
        }
    }

    fn apply_model_defaults(&self, body: &mut Value, model: &str, max_tokens: u32, debug: bool) {
        if let Some(obj) = body.as_object_mut() {
            obj.remove("max_tokens");
            obj.remove("max_completion_tokens");
            obj.insert(Self::max_tokens_param(model).to_string(), json!(max_tokens));
            match self.temperature_directive(model) {
                TemperatureDirective::Send(value) => {
                    obj.insert("temperature".into(), json!(value));
                }
                TemperatureDirective::Omit => {
                    obj.remove("temperature");
                }
            }
            if debug
                && Self::is_gpt5_model(model)
                && self.temperature_user_override
                && self.temperature_override.is_some()
            {
                eprintln!(
                    "[warn] Overriding requested temperature to 1.0 for GPT-5 model '{}'.",
                    model
                );
            }
            let reasoning = if Self::default_reasoning_effort(model).is_some() {
                self.reasoning_effort
                    .as_deref()
                    .or_else(|| Self::default_reasoning_effort(model))
            } else {
                None
            };
            if let Some(effort) = reasoning {
                obj.insert("reasoning_effort".into(), json!(effort));
            } else {
                obj.remove("reasoning_effort");
            }
        }
    }

    fn chat_url(&self) -> String {
        format!("{}/chat/completions", self.base_url.trim_end_matches('/'))
    }

    /// Non-streaming chat completion: returns the full assistant message.
    pub async fn chat_once(&self, model: &str, prompt: &str, debug: bool) -> Result<String> {
        let mut body = json!({
            "model": model,
            "messages": [
                {"role": "user", "content": prompt}
            ]
        });
        self.apply_model_defaults(&mut body, model, DEFAULT_MAX_COMPLETION_TOKENS, debug);
        if debug {
            let bytes = serde_json::to_vec(&body).unwrap();
            eprintln!("[debug] POST {} ({} bytes)", self.chat_url(), bytes.len());
        }
        let resp = self
            .request_builder()
            .json(&body)
            .send()
            .await
            .with_context(|| "HTTP request failed")?;
        let status = resp.status();
        let text = resp.text().await?;
        if !status.is_success() {
            return Err(anyhow!("API error ({}): {}", status, text));
        }
        let parsed: ChatResponse = serde_json::from_str(&text)
            .with_context(|| format!("Failed to parse chat response JSON: {}", text))?;
        let choice = parsed
            .choices
            .into_iter()
            .next()
            .ok_or_else(|| anyhow!("No choices in response"))?;
        Ok(choice.message.content)
    }

    /// Non-streaming chat completion using explicit messages.
    pub async fn chat_once_messages(
        &self,
        model: &str,
        messages: &[Msg<'_>],
        debug: bool,
    ) -> Result<String> {
        let mut body = json!({
            "model": model,
            "messages": messages
        });
        self.apply_model_defaults(&mut body, model, DEFAULT_MAX_COMPLETION_TOKENS, debug);
        if debug {
            let bytes = serde_json::to_vec(&body).unwrap();
            eprintln!("[debug] POST {} ({} bytes)", self.chat_url(), bytes.len());
        }
        let resp = self
            .request_builder()
            .json(&body)
            .send()
            .await
            .with_context(|| "HTTP request failed")?;
        let status = resp.status();
        let text = resp.text().await?;
        if !status.is_success() {
            return Err(anyhow!("API error ({}): {}", status, text));
        }
        let parsed: ChatResponse = serde_json::from_str(&text)
            .with_context(|| format!("Failed to parse chat response JSON: {}", text))?;
        let choice = parsed
            .choices
            .into_iter()
            .next()
            .ok_or_else(|| anyhow!("No choices in response"))?;
        Ok(choice.message.content)
    }

    /// Non-streaming chat completion allowing tool specs; returns either content or a tool call.
    pub async fn chat_once_messages_with_tools(
        &self,
        model: &str,
        messages: &[Msg<'_>],
        tools: serde_json::Value,
        debug: bool,
    ) -> Result<AssistantReply> {
        let mut body = json!({
            "model": model,
            "messages": messages,
            "tools": tools
        });
        self.apply_model_defaults(&mut body, model, DEFAULT_MAX_COMPLETION_TOKENS, debug);
        if debug {
            let bytes = serde_json::to_vec(&body).unwrap();
            eprintln!("[debug] POST {} ({} bytes)", self.chat_url(), bytes.len());
        }
        let resp = self
            .request_builder()
            .json(&body)
            .send()
            .await
            .with_context(|| "HTTP request failed")?;
        let status = resp.status();
        let text = resp.text().await?;
        if !status.is_success() {
            return Err(anyhow!("API error ({}): {}", status, text));
        }

        // Try to parse as tool-aware response first
        let parsed_tools: ChatResponseWithTools = serde_json::from_str(&text)
            .with_context(|| format!("Failed to parse tool-aware chat response JSON: {}", text))?;
        let choice = parsed_tools
            .choices
            .into_iter()
            .next()
            .ok_or_else(|| anyhow!("No choices in response"))?;

        if let Some(calls) = choice.message.tool_calls {
            if let Some(first) = calls.into_iter().next() {
                return Ok(AssistantReply::ToolCall {
                    name: first.function.name,
                    arguments_json: first.function.arguments,
                });
            }
        }
        let content = choice.message.content.unwrap_or_default();
        Ok(AssistantReply::Content(content))
    }

    /// Streaming chat completion. Calls `on_token` for each token/delta of content.
    pub async fn chat_stream<F>(
        &self,
        model: &str,
        prompt: &str,
        debug: bool,
        mut on_token: F,
    ) -> Result<()>
    where
        F: FnMut(&str),
    {
        let mut body = json!({
            "model": model,
            "messages": [
                {"role": "user", "content": prompt}
            ],
            "stream": true
        });
        self.apply_model_defaults(&mut body, model, DEFAULT_MAX_COMPLETION_TOKENS, debug);
        if debug {
            let bytes = serde_json::to_vec(&body).unwrap();
            eprintln!(
                "[debug] POST {} ({} bytes, stream)",
                self.chat_url(),
                bytes.len()
            );
        }
        let resp = self
            .request_builder()
            .json(&body)
            .send()
            .await
            .with_context(|| "HTTP request failed")?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(anyhow!("API error ({}): {}", status, text));
        }

        // The OpenAI-compatible API returns text/event-stream with lines prefixed by "data: ".
        // We read chunks and split by newlines; we accumulate and parse JSON lines.
        let mut stream = resp.bytes_stream();
        let mut buffer = Vec::<u8>::new();

        while let Some(item) = stream.next().await {
            let chunk: Bytes = item?;
            buffer.extend_from_slice(&chunk);
            // Process complete lines
            while let Some(pos) =
                find_double_newline(&buffer).or_else(|| find_single_newline(&buffer))
            {
                let line = buffer.drain(..=pos).collect::<Vec<u8>>();
                let s = String::from_utf8_lossy(&line);
                for raw in s.split('\n') {
                    let data = raw.trim();
                    if data.is_empty() {
                        continue;
                    }
                    if let Some(rest) = data.strip_prefix("data: ") {
                        if rest == "[DONE]" {
                            return Ok(());
                        }
                        if let Ok(parsed) = serde_json::from_str::<ChatStreamChunk>(rest) {
                            for c in parsed.choices.into_iter() {
                                if let Some(delta) = c.delta {
                                    if let Some(token) = delta.content {
                                        on_token(&token);
                                    }
                                }
                            }
                        } else if debug {
                            eprintln!("[debug] Unparsed stream line: {}", rest);
                        }
                    }
                }
            }
        }

        Ok(())
    }

    /// Streaming chat completion with explicit messages (supports system+user for qq).
    pub async fn chat_stream_messages<F>(
        &self,
        model: &str,
        messages: &[Msg<'_>],
        debug: bool,
        mut on_token: F,
    ) -> Result<()>
    where
        F: FnMut(&str),
    {
        let mut body = json!({
            "model": model,
            "messages": messages,
            "stream": true
        });
        self.apply_model_defaults(&mut body, model, DEFAULT_MAX_COMPLETION_TOKENS, debug);
        if debug {
            let bytes = serde_json::to_vec(&body).unwrap();
            eprintln!(
                "[debug] POST {} ({} bytes, stream)",
                self.chat_url(),
                bytes.len()
            );
        }
        let resp = self
            .request_builder()
            .json(&body)
            .send()
            .await
            .with_context(|| "HTTP request failed")?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(anyhow!("API error ({}): {}", status, text));
        }

        // Reuse the same SSE parsing as non-message streaming
        let mut stream = resp.bytes_stream();
        let mut buffer = Vec::<u8>::new();

        while let Some(item) = stream.next().await {
            let chunk: Bytes = item?;
            buffer.extend_from_slice(&chunk);
            while let Some(pos) =
                find_double_newline(&buffer).or_else(|| find_single_newline(&buffer))
            {
                let line = buffer.drain(..=pos).collect::<Vec<u8>>();
                let s = String::from_utf8_lossy(&line);
                for raw in s.split('\n') {
                    let data = raw.trim();
                    if data.is_empty() {
                        continue;
                    }
                    if let Some(rest) = data.strip_prefix("data: ") {
                        if rest == "[DONE]" {
                            return Ok(());
                        }
                        if let Ok(parsed) = serde_json::from_str::<ChatStreamChunk>(rest) {
                            for c in parsed.choices.into_iter() {
                                if let Some(delta) = c.delta {
                                    if let Some(token) = delta.content {
                                        on_token(&token);
                                    }
                                }
                            }
                        } else if debug {
                            eprintln!("[debug] Unparsed stream line: {}", rest);
                        }
                    }
                }
            }
        }

        Ok(())
    }
}

fn load_root_certificates(path: &Path) -> Result<Vec<Certificate>> {
    let data = fs::read(path)
        .with_context(|| format!("Reading TLS certificate(s) at {}", path.display()))?;
    let mut cursor = Cursor::new(&data);
    let mut parsed = Vec::new();
    for pem in certs(&mut cursor) {
        let der =
            pem.with_context(|| format!("Parsing PEM certificate from {}", path.display()))?;
        parsed.push(
            Certificate::from_der(der.as_ref())
                .with_context(|| format!("Parsing PEM certificate from {}", path.display()))?,
        );
    }
    if !parsed.is_empty() {
        return Ok(parsed);
    }

    if let Ok(cert) = Certificate::from_pem(&data) {
        return Ok(vec![cert]);
    }

    let cert = Certificate::from_der(&data)
        .with_context(|| format!("Parsing DER certificate from {}", path.display()))?;
    Ok(vec![cert])
}

/// Minimal typed message for multi-message calls.
#[derive(Debug, Clone, serde::Serialize)]
pub struct Msg<'a> {
    pub role: &'a str,
    pub content: &'a str,
}

fn find_double_newline(buf: &[u8]) -> Option<usize> {
    // Find position to cut at a blank line (\n\n). Return index of the second newline.
    buf.windows(2).position(|w| w == b"\n\n").map(|i| i + 1)
}

fn find_single_newline(buf: &[u8]) -> Option<usize> {
    buf.iter().position(|&b| b == b'\n')
}

mod cli_backend {
    use super::*;
    use std::process::Stdio;
    use tokio::io::AsyncWriteExt;
    use tokio::process::Command;

    #[derive(Debug)]
    pub struct CliCompletionRequest<'a> {
        pub engine: CliEngine,
        pub binary: &'a str,
        pub base_args: &'a [String],
        pub system_prompt: &'a str,
        pub user_prompt: &'a str,
        pub model: &'a str,
        pub reasoning_effort: Option<&'a str>,
        pub debug: bool,
    }

    pub async fn run_cli_completion(req: CliCompletionRequest<'_>) -> Result<String> {
        match req.engine {
            CliEngine::Codex => run_codex(req).await,
        }
    }

    async fn run_codex(req: CliCompletionRequest<'_>) -> Result<String> {
        let mut cmd = Command::new(req.binary);
        if req.base_args.is_empty() {
            cmd.arg("exec");
        } else {
            cmd.args(req.base_args);
        }
        cmd.arg("--json");
        let reasoning = req.reasoning_effort.unwrap_or("minimal");
        cmd.arg("-c");
        cmd.arg(format!("model_reasoning_effort={}", reasoning));
        cmd.arg("-c");
        cmd.arg("sandbox_mode=read-only");
        cmd.arg("-c");
        cmd.arg("tools.web_search=false");
        cmd.arg("-");

        let mut prompt = String::new();
        prompt.push_str("<system-prompt>\n");
        prompt.push_str(req.system_prompt);
        prompt.push_str("\n</system-prompt>\n\n");
        prompt.push_str("<user-prompt>\n");
        prompt.push_str(req.user_prompt);
        prompt.push_str("\n</user-prompt>\n");

        if req.debug {
            eprintln!(
                "[debug] Running CLI provider '{}' with args: {:?}",
                req.binary, cmd
            );
        }

        let mut child = cmd
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .with_context(|| {
                format!(
                    "Failed to spawn CLI provider '{}'. Is it installed and on your PATH?",
                    req.binary
                )
            })?;

        if let Some(mut stdin) = child.stdin.take() {
            stdin
                .write_all(prompt.as_bytes())
                .await
                .context("Writing prompt to CLI provider stdin")?;
        }

        let output = child.wait_with_output().await?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let stdout = String::from_utf8_lossy(&output.stdout);
            return Err(anyhow!(
                "CLI provider '{}' exited with status {}.{}{}",
                req.binary,
                output
                    .status
                    .code()
                    .map(|c| c.to_string())
                    .unwrap_or_else(|| "signal".to_string()),
                if stdout.trim().is_empty() {
                    "".to_string()
                } else {
                    format!("\nstdout: {}", stdout.trim())
                },
                if stderr.trim().is_empty() {
                    "".to_string()
                } else {
                    format!("\nstderr: {}", stderr.trim())
                }
            ));
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        parse_codex_response(&stdout)
    }

    #[derive(Debug, Deserialize)]
    struct CodexEvent {
        #[serde(rename = "type")]
        event_type: String,
        #[serde(default)]
        item: Option<CodexItem>,
    }

    #[derive(Debug, Deserialize)]
    struct CodexItem {
        #[serde(rename = "type")]
        item_type: String,
        #[serde(default)]
        text: Option<String>,
    }

    fn parse_codex_response(stdout: &str) -> Result<String> {
        let mut messages = Vec::new();
        for line in stdout.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            match serde_json::from_str::<CodexEvent>(trimmed) {
                Ok(event) => {
                    if event.event_type == "item.completed" {
                        if let Some(item) = event.item {
                            if item.item_type == "agent_message" {
                                if let Some(text) = item.text {
                                    if !text.trim().is_empty() {
                                        messages.push(text);
                                    }
                                }
                            }
                        }
                    }
                }
                Err(_) => {
                    continue;
                }
            }
        }
        if messages.is_empty() {
            return Err(anyhow!("CLI provider returned no agent_message text."));
        }
        Ok(messages.join("\n\n"))
    }

    #[cfg(test)]
    pub(super) fn parse_codex_response_for_test(input: &str) -> Result<String> {
        parse_codex_response(input)
    }
}

pub use cli_backend::{CliCompletionRequest, run_cli_completion};

#[cfg(test)]
mod tests {
    use super::cli_backend::parse_codex_response_for_test;
    use super::load_root_certificates;
    use rcgen::{CertifiedKey, generate_simple_self_signed};
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn load_root_certificates_supports_multiple_pem_entries() {
        let bundle_dir = tempdir().unwrap();
        let CertifiedKey { cert: cert_a, .. } =
            generate_simple_self_signed(["a.lan".into()]).unwrap();
        let CertifiedKey { cert: cert_b, .. } =
            generate_simple_self_signed(["b.lan".into()]).unwrap();
        let pem = format!("{}\n{}", cert_a.pem(), cert_b.pem());
        let path = bundle_dir.path().join("bundle.pem");
        fs::write(&path, pem).unwrap();

        let certs = load_root_certificates(&path).expect("certs load");
        assert_eq!(certs.len(), 2);
    }

    #[test]
    fn load_root_certificates_handles_der_files() {
        let dir = tempdir().unwrap();
        let CertifiedKey { cert, .. } = generate_simple_self_signed(["der.test".into()]).unwrap();
        let der = cert.der().as_ref().to_vec();
        let path = dir.path().join("bundle.der");
        fs::write(&path, der).unwrap();

        let certs = load_root_certificates(&path).expect("certs load");
        assert_eq!(certs.len(), 1);
    }

    #[test]
    fn codex_parser_returns_last_agent_message() {
        let payload = r#"{"type":"item.completed","item":{"id":"item_0","type":"reasoning","text":"Reasoning"}}
{"type":"item.completed","item":{"id":"item_1","type":"agent_message","text":"First"}}
{"type":"item.completed","item":{"id":"item_2","type":"agent_message","text":"Second"}}"#;
        let merged = payload.replace('\n', "\n");
        let parsed = parse_codex_response_for_test(&merged).expect("parse");
        assert_eq!(parsed, "First\n\nSecond");
    }
}
