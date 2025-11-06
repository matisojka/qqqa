use anyhow::{Context, Result, anyhow};
use bytes::Bytes;
use futures_util::StreamExt;
use reqwest::Client;
use serde::Deserialize;
use serde_json::json;
use std::time::Duration;

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
}

impl ChatClient {
    pub fn new(base_url: String, api_key: String) -> Result<Self> {
        // Use rustls for TLS; set useful timeouts for robustness.
        let client = Client::builder()
            .timeout(Duration::from_secs(60))
            .connect_timeout(Duration::from_secs(10))
            .build()?;
        Ok(Self {
            client,
            base_url,
            api_key,
        })
    }

    fn chat_url(&self) -> String {
        format!("{}/chat/completions", self.base_url.trim_end_matches('/'))
    }

    /// Determine the correct max tokens parameter name based on the model.
    /// Newer OpenAI models (gpt-5, o1, o3 series) use "max_completion_tokens",
    /// while older models use "max_tokens".
    fn max_tokens_param(model: &str) -> &'static str {
        let model_lower = model.to_lowercase();
        if model_lower.starts_with("gpt-5")
            || model_lower.starts_with("o1")
            || model_lower.starts_with("o3")
        {
            "max_completion_tokens"
        } else {
            "max_tokens"
        }
    }

    /// Check if the model supports custom temperature values.
    /// Newer OpenAI models (gpt-5, o1, o3 series) only support the default temperature (1.0).
    fn supports_temperature(model: &str) -> bool {
        let model_lower = model.to_lowercase();
        !(model_lower.starts_with("gpt-5")
            || model_lower.starts_with("o1")
            || model_lower.starts_with("o3"))
    }

    /// Non-streaming chat completion: returns the full assistant message.
    pub async fn chat_once(&self, model: &str, prompt: &str, debug: bool) -> Result<String> {
        let max_tokens_key = Self::max_tokens_param(model);
        let mut body = json!({
            "model": model,
            "messages": [
                {"role": "user", "content": prompt}
            ],
            max_tokens_key: 4000
        });
        if Self::supports_temperature(model) {
            body["temperature"] = json!(0.0);
        }
        if debug {
            let bytes = serde_json::to_vec(&body).unwrap();
            eprintln!("[debug] POST {} ({} bytes)", self.chat_url(), bytes.len());
        }
        let resp = self
            .client
            .post(self.chat_url())
            .bearer_auth(&self.api_key)
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
        let max_tokens_key = Self::max_tokens_param(model);
        let mut body = json!({
            "model": model,
            "messages": messages,
            max_tokens_key: 4000
        });
        if Self::supports_temperature(model) {
            body["temperature"] = json!(0.0);
        }
        if debug {
            let bytes = serde_json::to_vec(&body).unwrap();
            eprintln!("[debug] POST {} ({} bytes)", self.chat_url(), bytes.len());
        }
        let resp = self
            .client
            .post(self.chat_url())
            .bearer_auth(&self.api_key)
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
        let max_tokens_key = Self::max_tokens_param(model);
        let mut body = json!({
            "model": model,
            "messages": messages,
            "tools": tools,
            max_tokens_key: 4000
        });
        if Self::supports_temperature(model) {
            body["temperature"] = json!(0.0);
        }
        if debug {
            let bytes = serde_json::to_vec(&body).unwrap();
            eprintln!("[debug] POST {} ({} bytes)", self.chat_url(), bytes.len());
        }
        let resp = self
            .client
            .post(self.chat_url())
            .bearer_auth(&self.api_key)
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
        let max_tokens_key = Self::max_tokens_param(model);
        let mut body = json!({
            "model": model,
            "messages": [
                {"role": "user", "content": prompt}
            ],
            "stream": true,
            max_tokens_key: 4000
        });
        if Self::supports_temperature(model) {
            body["temperature"] = json!(0.0);
        }
        if debug {
            let bytes = serde_json::to_vec(&body).unwrap();
            eprintln!(
                "[debug] POST {} ({} bytes, stream)",
                self.chat_url(),
                bytes.len()
            );
        }
        let resp = self
            .client
            .post(self.chat_url())
            .bearer_auth(&self.api_key)
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
        let max_tokens_key = Self::max_tokens_param(model);
        let mut body = json!({
            "model": model,
            "messages": messages,
            "stream": true,
            max_tokens_key: 4000
        });
        if Self::supports_temperature(model) {
            body["temperature"] = json!(0.0);
        }
        if debug {
            let bytes = serde_json::to_vec(&body).unwrap();
            eprintln!(
                "[debug] POST {} ({} bytes, stream)",
                self.chat_url(),
                bytes.len()
            );
        }
        let resp = self
            .client
            .post(self.chat_url())
            .bearer_auth(&self.api_key)
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
