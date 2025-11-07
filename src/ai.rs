use anyhow::{Context, Result, anyhow};
use bytes::Bytes;
use futures_util::StreamExt;
use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use reqwest::{Client, RequestBuilder};
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::time::Duration;

const DEFAULT_MAX_COMPLETION_TOKENS: u32 = 4000;

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

pub struct ChatClient {
    client: Client,
    base_url: String,
    api_key: String,
    reasoning_effort: Option<String>,
    default_headers: HeaderMap,
}

impl ChatClient {
    pub fn new(
        base_url: String,
        api_key: String,
        headers: HashMap<String, String>,
    ) -> Result<Self> {
        // Use rustls for TLS; set useful timeouts for robustness.
        let client = Client::builder()
            .timeout(Duration::from_secs(60))
            .connect_timeout(Duration::from_secs(10))
            .build()?;
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
            default_headers,
        })
    }

    pub fn with_reasoning_effort(mut self, reasoning_effort: Option<String>) -> Self {
        self.reasoning_effort = reasoning_effort;
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

    fn max_tokens_param(model: &str) -> &'static str {
        if Self::is_new_style_model(model) {
            "max_completion_tokens"
        } else {
            "max_tokens"
        }
    }

    fn supports_temperature(model: &str) -> bool {
        !Self::is_new_style_model(model)
    }

    fn default_reasoning_effort(model: &str) -> Option<&'static str> {
        if model.to_ascii_lowercase().starts_with("gpt-5") {
            Some("minimal")
        } else {
            None
        }
    }

    fn apply_model_defaults(&self, body: &mut Value, model: &str, max_tokens: u32) {
        if let Some(obj) = body.as_object_mut() {
            obj.remove("max_tokens");
            obj.remove("max_completion_tokens");
            obj.insert(Self::max_tokens_param(model).to_string(), json!(max_tokens));
            if Self::supports_temperature(model) {
                obj.insert("temperature".into(), json!(0.0));
            } else {
                obj.remove("temperature");
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
        self.apply_model_defaults(&mut body, model, DEFAULT_MAX_COMPLETION_TOKENS);
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
        self.apply_model_defaults(&mut body, model, DEFAULT_MAX_COMPLETION_TOKENS);
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
        self.apply_model_defaults(&mut body, model, DEFAULT_MAX_COMPLETION_TOKENS);
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
        self.apply_model_defaults(&mut body, model, DEFAULT_MAX_COMPLETION_TOKENS);
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
        self.apply_model_defaults(&mut body, model, DEFAULT_MAX_COMPLETION_TOKENS);
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
