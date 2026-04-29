use std::io::{BufRead, BufReader};
use std::sync::mpsc;

use anyhow::{Context, Result, anyhow, bail};
use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::config::{LlmConfig, ProviderKind, api_url};

/// A streaming chunk sender. The LLM client sends partial text through this.
pub type StreamSink = mpsc::Sender<String>;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tool_calls: Vec<ToolCall>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tool_results: Vec<ToolResult>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub args: Value,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct ToolResult {
    pub tool_use_id: String,
    pub content: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct AssistantResponse {
    pub content: String,
    #[serde(default)]
    pub tool_calls: Vec<ToolCall>,
    pub raw: Value,
}

pub trait LlmClient {
    fn chat(&mut self, messages: &[ChatMessage], tools: &Value) -> Result<AssistantResponse>;
    fn name(&self) -> &str;

    /// Set a channel sender for streaming partial text. Default is no-op.
    fn set_stream_sink(&mut self, _sink: Option<StreamSink>) {}
}

pub enum AnyLlmClient {
    Mock(MockClient),
    OpenAi(OpenAiClient),
    Anthropic(AnthropicClient),
}

impl AnyLlmClient {
    pub fn from_config(cfg: LlmConfig) -> Result<Self> {
        Ok(match cfg.provider {
            ProviderKind::Mock => Self::Mock(MockClient::default()),
            ProviderKind::OpenAi => Self::OpenAi(OpenAiClient::new(cfg)?),
            ProviderKind::Anthropic => Self::Anthropic(AnthropicClient::new(cfg)?),
        })
    }
}

impl LlmClient for AnyLlmClient {
    fn chat(&mut self, messages: &[ChatMessage], tools: &Value) -> Result<AssistantResponse> {
        match self {
            Self::Mock(c) => c.chat(messages, tools),
            Self::OpenAi(c) => c.chat(messages, tools),
            Self::Anthropic(c) => c.chat(messages, tools),
        }
    }

    fn name(&self) -> &str {
        match self {
            Self::Mock(c) => c.name(),
            Self::OpenAi(c) => c.name(),
            Self::Anthropic(c) => c.name(),
        }
    }

    fn set_stream_sink(&mut self, sink: Option<StreamSink>) {
        match self {
            Self::Mock(c) => c.set_stream_sink(sink),
            Self::OpenAi(c) => c.set_stream_sink(sink),
            Self::Anthropic(c) => c.set_stream_sink(sink),
        }
    }
}

#[derive(Default)]
pub struct MockClient {
    turns: usize,
}

impl LlmClient for MockClient {
    fn chat(&mut self, messages: &[ChatMessage], _tools: &Value) -> Result<AssistantResponse> {
        self.turns += 1;
        let last = messages.last().map(|m| m.content.as_str()).unwrap_or("");
        Ok(AssistantResponse {
            content: format!("[mock] no provider configured. Last input: {last}"),
            tool_calls: Vec::new(),
            raw: json!({"provider":"mock","turns":self.turns}),
        })
    }

    fn name(&self) -> &str {
        "mock"
    }
}

pub struct OpenAiClient {
    cfg: LlmConfig,
    http: Client,
    stream_sink: Option<StreamSink>,
}

impl OpenAiClient {
    pub fn new(cfg: LlmConfig) -> Result<Self> {
        if cfg.api_key.as_deref().unwrap_or_default().is_empty() {
            bail!("OpenAI provider selected but OPENAI_API_KEY/RGA_OPENAI_API_KEY is not set");
        }
        Ok(Self {
            cfg,
            http: Client::new(),
            stream_sink: None,
        })
    }
}

impl LlmClient for OpenAiClient {
    fn chat(&mut self, messages: &[ChatMessage], tools: &Value) -> Result<AssistantResponse> {
        let url = api_url(&self.cfg.api_base, "chat/completions");
        let mut oai_messages = Vec::new();
        for msg in messages {
            if msg.role == "system" {
                oai_messages.push(json!({"role": "system", "content": msg.content}));
                continue;
            }
            for tr in &msg.tool_results {
                oai_messages.push(json!({
                    "role": "tool",
                    "tool_call_id": tr.tool_use_id,
                    "content": tr.content,
                }));
            }
            if !msg.content.is_empty() || msg.role == "assistant" {
                let mut item = json!({"role": msg.role, "content": msg.content});
                if !msg.tool_calls.is_empty() {
                    item["tool_calls"] = json!(
                        msg.tool_calls
                            .iter()
                            .map(|tc| json!({
                                "id": tc.id,
                                "type": "function",
                                "function": {"name": tc.name, "arguments": tc.args.to_string()},
                            }))
                            .collect::<Vec<_>>()
                    );
                }
                oai_messages.push(item);
            }
        }

        let use_stream = self.stream_sink.is_some();
        let mut payload = json!({
            "model": self.cfg.model,
            "messages": oai_messages,
            "stream": use_stream,
        });
        if let Some(t) = self.cfg.temperature {
            payload["temperature"] = json!(t);
        }
        if let Some(mt) = self.cfg.max_tokens {
            payload["max_completion_tokens"] = json!(mt);
        }
        if let Some(re) = &self.cfg.reasoning_effort {
            payload["reasoning_effort"] = json!(re);
        }
        if tools.as_array().map(|a| !a.is_empty()).unwrap_or(false) {
            payload["tools"] = tools.clone();
        }

        validate_https_url(&url)?;

        if use_stream {
            let resp = self
                .http
                .post(url)
                .bearer_auth(self.cfg.api_key.as_deref().unwrap())
                .json(&payload)
                .send()
                .context("send OpenAI streaming request")?
                .error_for_status()
                .context("OpenAI streaming status")?;
            parse_openai_stream(resp, self.stream_sink.as_ref())
        } else {
            let data: Value = self
                .http
                .post(url)
                .bearer_auth(self.cfg.api_key.as_deref().unwrap())
                .json(&payload)
                .send()
                .context("send OpenAI request")?
                .error_for_status()
                .context("OpenAI status")?
                .json()
                .context("parse OpenAI response")?;
            parse_openai_response(data)
        }
    }

    fn name(&self) -> &str {
        "openai"
    }

    fn set_stream_sink(&mut self, sink: Option<StreamSink>) {
        self.stream_sink = sink;
    }
}

/// Parse an OpenAI SSE stream. Each line is `data: {json}` or `data: [DONE]`.
fn parse_openai_stream(
    resp: reqwest::blocking::Response,
    sink: Option<&StreamSink>,
) -> Result<AssistantResponse> {
    let reader = BufReader::new(resp);
    let mut full_content = String::new();
    let mut tool_calls_raw: Vec<(String, String, String)> = Vec::new(); // (id, name, args_buf)
    let mut raw_json = json!({});

    for line in reader.lines() {
        let line = line.context("read SSE line")?;
        let line = line.trim();
        if line.is_empty() || line.starts_with(':') {
            continue;
        }
        if !line.starts_with("data: ") {
            continue;
        }
        let data = &line[6..];
        if data == "[DONE]" {
            break;
        }
        let chunk: Value = match serde_json::from_str(data) {
            Ok(v) => v,
            Err(_) => continue,
        };
        raw_json = chunk.clone();

        if let Some(delta) = chunk.pointer("/choices/0/delta") {
            // Content delta
            if let Some(content) = delta.get("content").and_then(Value::as_str) {
                full_content.push_str(content);
                if let Some(s) = sink {
                    let _ = s.send(content.to_string());
                }
            }
            // Tool call deltas
            if let Some(tc_arr) = delta.get("tool_calls").and_then(Value::as_array) {
                for tc in tc_arr {
                    let idx = tc.get("index").and_then(Value::as_u64).unwrap_or(0) as usize;
                    let id = tc
                        .get("id")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_string();
                    let name = tc
                        .pointer("/function/name")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_string();
                    let args_delta = tc
                        .pointer("/function/arguments")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_string();
                    while tool_calls_raw.len() <= idx {
                        tool_calls_raw.push((String::new(), String::new(), String::new()));
                    }
                    if !id.is_empty() {
                        tool_calls_raw[idx].0 = id;
                    }
                    if !name.is_empty() {
                        tool_calls_raw[idx].1 = name;
                    }
                    tool_calls_raw[idx].2.push_str(&args_delta);
                }
            }
        }
    }

    let mut calls = Vec::new();
    for (idx, (id, name, raw_args)) in tool_calls_raw.into_iter().enumerate() {
        if name.is_empty() {
            continue;
        }
        let args = serde_json::from_str(&raw_args).unwrap_or_else(|_| json!({"_raw": raw_args}));
        calls.push(ToolCall {
            id: if id.is_empty() {
                format!("call_{idx}")
            } else {
                id
            },
            name,
            args,
        });
    }

    Ok(AssistantResponse {
        content: full_content,
        tool_calls: calls,
        raw: raw_json,
    })
}

fn parse_openai_response(data: Value) -> Result<AssistantResponse> {
    let msg = data
        .pointer("/choices/0/message")
        .ok_or_else(|| anyhow!("OpenAI response missing choices[0].message"))?;
    let content = msg
        .get("content")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let mut calls = Vec::new();
    if let Some(arr) = msg.get("tool_calls").and_then(Value::as_array) {
        for (idx, tc) in arr.iter().enumerate() {
            let name = tc
                .pointer("/function/name")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            let raw_args = tc
                .pointer("/function/arguments")
                .and_then(Value::as_str)
                .unwrap_or("{}");
            let args = serde_json::from_str(raw_args).unwrap_or_else(|_| json!({"_raw": raw_args}));
            calls.push(ToolCall {
                id: tc
                    .get("id")
                    .and_then(Value::as_str)
                    .map(str::to_string)
                    .unwrap_or_else(|| format!("call_{idx}")),
                name,
                args,
            });
        }
    }
    Ok(AssistantResponse {
        content,
        tool_calls: calls,
        raw: data,
    })
}

fn validate_https_url(url: &str) -> Result<()> {
    if url.starts_with("https://")
        || std::env::var("RGA_ALLOW_INSECURE_PROVIDER").as_deref() == Ok("1")
    {
        Ok(())
    } else {
        bail!(
            "provider URL must use https:// (set RGA_ALLOW_INSECURE_PROVIDER=1 for local testing)"
        );
    }
}

pub struct AnthropicClient {
    cfg: LlmConfig,
    http: Client,
    stream_sink: Option<StreamSink>,
}

impl AnthropicClient {
    pub fn new(cfg: LlmConfig) -> Result<Self> {
        if cfg.api_key.as_deref().unwrap_or_default().is_empty() {
            bail!(
                "Anthropic provider selected but ANTHROPIC_API_KEY/RGA_ANTHROPIC_API_KEY is not set"
            );
        }
        Ok(Self {
            cfg,
            http: Client::new(),
            stream_sink: None,
        })
    }
}

impl LlmClient for AnthropicClient {
    fn chat(&mut self, messages: &[ChatMessage], tools: &Value) -> Result<AssistantResponse> {
        let url = api_url(&self.cfg.api_base, "messages");
        let mut system = String::new();
        let mut claude_messages = Vec::new();
        for msg in messages {
            if msg.role == "system" {
                system.push_str(&msg.content);
                continue;
            }
            let mut content = Vec::new();
            for tr in &msg.tool_results {
                content.push(json!({"type":"tool_result", "tool_use_id": tr.tool_use_id, "content": tr.content}));
            }
            if !msg.content.is_empty() {
                content.push(json!({"type":"text","text": msg.content}));
            }
            if msg.role == "assistant" {
                for tc in &msg.tool_calls {
                    content.push(
                        json!({"type":"tool_use", "id": tc.id, "name": tc.name, "input": tc.args}),
                    );
                }
            }
            if !content.is_empty() {
                claude_messages.push(json!({"role": if msg.role == "assistant" {"assistant"} else {"user"}, "content": content}));
            }
        }

        let use_stream = self.stream_sink.is_some();
        let mut payload = json!({
            "model": self.cfg.model,
            "messages": claude_messages,
            "max_tokens": self.cfg.max_tokens.unwrap_or(8192),
            "stream": use_stream,
        });
        if !system.is_empty() {
            payload["system"] = json!(system);
        }
        if let Some(t) = self.cfg.temperature {
            payload["temperature"] = json!(t);
        }
        let converted = openai_tools_to_anthropic(tools);
        if converted.as_array().map(|a| !a.is_empty()).unwrap_or(false) {
            payload["tools"] = converted;
        }

        validate_https_url(&url)?;
        let key = self.cfg.api_key.as_deref().unwrap();
        let mut req = self
            .http
            .post(&url)
            .header("anthropic-version", "2023-06-01")
            .json(&payload);
        if key.starts_with("sk-ant-") {
            req = req.header("x-api-key", key);
        } else {
            req = req.bearer_auth(key);
        }

        if use_stream {
            let resp = req
                .send()
                .context("send Anthropic streaming request")?
                .error_for_status()
                .context("Anthropic streaming status")?;
            parse_anthropic_stream(resp, self.stream_sink.as_ref())
        } else {
            let data: Value = req
                .send()
                .context("send Anthropic request")?
                .error_for_status()
                .context("Anthropic status")?
                .json()
                .context("parse Anthropic response")?;
            parse_anthropic_response(data)
        }
    }

    fn name(&self) -> &str {
        "anthropic"
    }

    fn set_stream_sink(&mut self, sink: Option<StreamSink>) {
        self.stream_sink = sink;
    }
}

/// Parse Anthropic SSE stream.
fn parse_anthropic_stream(
    resp: reqwest::blocking::Response,
    sink: Option<&StreamSink>,
) -> Result<AssistantResponse> {
    let reader = BufReader::new(resp);
    let mut full_content = String::new();
    let mut tool_calls: Vec<ToolCall> = Vec::new();
    let mut current_tool_idx: usize = 0;
    let mut current_tool_args = String::new();
    let mut in_tool_use = false;

    for line in reader.lines() {
        let line = line.context("read SSE line")?;
        let line = line.trim();
        if line.is_empty() || line.starts_with(':') {
            continue;
        }
        if !line.starts_with("data: ") {
            continue;
        }
        let data = &line[6..];
        let event: Value = match serde_json::from_str(data) {
            Ok(v) => v,
            Err(_) => continue,
        };

        let etype = event.get("type").and_then(Value::as_str).unwrap_or("");
        match etype {
            "content_block_start" => {
                if let Some(cb) = event.get("content_block") {
                    if cb.get("type").and_then(Value::as_str) == Some("tool_use") {
                        in_tool_use = true;
                        current_tool_idx = tool_calls.len();
                        current_tool_args.clear();
                        tool_calls.push(ToolCall {
                            id: cb
                                .get("id")
                                .and_then(Value::as_str)
                                .unwrap_or("")
                                .to_string(),
                            name: cb
                                .get("name")
                                .and_then(Value::as_str)
                                .unwrap_or("")
                                .to_string(),
                            args: json!({}),
                        });
                    }
                }
            }
            "content_block_delta" => {
                if let Some(delta) = event.get("delta") {
                    if let Some(text) = delta.get("text").and_then(Value::as_str) {
                        full_content.push_str(text);
                        if let Some(s) = sink {
                            let _ = s.send(text.to_string());
                        }
                    }
                    if in_tool_use {
                        if let Some(partial) =
                            delta.get("partial_json").and_then(Value::as_str)
                        {
                            current_tool_args.push_str(partial);
                        }
                    }
                }
            }
            "content_block_stop" => {
                if in_tool_use && current_tool_idx < tool_calls.len() {
                    tool_calls[current_tool_idx].args =
                        serde_json::from_str(&current_tool_args)
                            .unwrap_or_else(|_| json!({"_raw": current_tool_args}));
                    in_tool_use = false;
                }
            }
            "message_stop" => break,
            _ => {}
        }
    }

    Ok(AssistantResponse {
        content: full_content,
        tool_calls,
        raw: json!({"provider": "anthropic", "stream": true}),
    })
}

fn openai_tools_to_anthropic(tools: &Value) -> Value {
    let mut out = Vec::new();
    if let Some(arr) = tools.as_array() {
        for t in arr {
            let f = t.get("function").unwrap_or(t);
            if let Some(name) = f.get("name").and_then(Value::as_str) {
                out.push(json!({
                    "name": name,
                    "description": f.get("description").and_then(Value::as_str).unwrap_or(""),
                    "input_schema": f.get("parameters").cloned().unwrap_or_else(|| json!({"type":"object","properties":{}})),
                }));
            }
        }
    }
    Value::Array(out)
}

fn parse_anthropic_response(data: Value) -> Result<AssistantResponse> {
    let mut content = String::new();
    let mut calls = Vec::new();
    if let Some(arr) = data.get("content").and_then(Value::as_array) {
        for (idx, b) in arr.iter().enumerate() {
            match b.get("type").and_then(Value::as_str).unwrap_or("") {
                "text" => content.push_str(b.get("text").and_then(Value::as_str).unwrap_or("")),
                "tool_use" => calls.push(ToolCall {
                    id: b
                        .get("id")
                        .and_then(Value::as_str)
                        .map(str::to_string)
                        .unwrap_or_else(|| format!("toolu_{idx}")),
                    name: b
                        .get("name")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_string(),
                    args: b.get("input").cloned().unwrap_or_else(|| json!({})),
                }),
                _ => {}
            }
        }
    }
    Ok(AssistantResponse {
        content,
        tool_calls: calls,
        raw: data,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_openai_tool_call() {
        let data = json!({"choices":[{"message":{"content":"hi","tool_calls":[{"id":"1","function":{"name":"file_read","arguments":"{\"path\":\"a\"}"}}]}}]});
        let r = parse_openai_response(data).unwrap();
        assert_eq!(r.content, "hi");
        assert_eq!(r.tool_calls[0].name, "file_read");
        assert_eq!(r.tool_calls[0].args["path"], "a");
    }

    #[test]
    fn converts_tools_for_anthropic() {
        let tools = json!([{"type":"function","function":{"name":"x","description":"d","parameters":{"type":"object"}}}]);
        let c = openai_tools_to_anthropic(&tools);
        assert_eq!(c[0]["name"], "x");
        assert_eq!(c[0]["input_schema"]["type"], "object");
    }
}
