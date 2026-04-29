use std::sync::mpsc;

use anyhow::Result;
use serde_json::{Value, json};

use crate::llm::{AssistantResponse, ChatMessage, LlmClient, ToolResult};
use crate::session_log;
use crate::tools::ToolDispatcher;

#[derive(Debug)]
pub struct AgentRunResult {
    pub result: String,
    pub turns: usize,
    pub exit_reason: String,
}

/// Run the agent loop. If `stream_sink` is provided, LLM responses are streamed through it.
pub fn agent_runner_loop<C: LlmClient>(
    client: &mut C,
    system_prompt: String,
    user_input: String,
    handler: &mut ToolDispatcher,
    tools_schema: Value,
    max_turns: usize,
    verbose: bool,
    stream_sink: Option<mpsc::Sender<String>>,
) -> Result<AgentRunResult> {
    let mut messages = vec![
        ChatMessage {
            role: "system".to_string(),
            content: system_prompt,
            tool_calls: Vec::new(),
            tool_results: Vec::new(),
        },
        ChatMessage {
            role: "user".to_string(),
            content: user_input.clone(),
            tool_calls: Vec::new(),
            tool_results: Vec::new(),
        },
    ];
    handler.push_history(format!(
        "[USER]: {}",
        smart_format(&user_input.replace('\n', " "), 200)
    ));
    let mut final_text = String::new();

    // Set streaming on the client for this run
    client.set_stream_sink(stream_sink);

    for turn in 1..=max_turns {
        if verbose {
            println!("**LLM Running (Turn {turn}) with {} ...**\n", client.name());
        }
        let response = client.chat(&messages, &tools_schema)?;
        let prompt_log = messages.last().map(|m| json!(m)).unwrap_or(Value::Null);
        let response_log = json!({
            "content": response.content,
            "tool_calls": response.tool_calls,
            "raw": response.raw,
        });
        let _ = session_log::append_turn(&handler.paths.temp, &prompt_log, &response_log);
        if !response.content.trim().is_empty() {
            print!("{}", response.content);
            if !response.content.ends_with('\n') {
                println!();
            }
        }
        let tool_calls = if response.tool_calls.is_empty() {
            vec![crate::llm::ToolCall {
                id: String::new(),
                name: "no_tool".to_string(),
                args: serde_json::json!({}),
            }]
        } else {
            response.tool_calls.clone()
        };

        let mut tool_results = Vec::new();
        let mut next_prompts = Vec::new();
        let mut exit_reason = None;
        for (idx, call) in tool_calls.iter().enumerate() {
            if call.name != "no_tool" && verbose {
                println!(
                    "🛠️ Tool: `{}` args:\n{}",
                    call.name,
                    serde_json::to_string_pretty(&call.args).unwrap_or_default()
                );
            }
            let outcome = handler.dispatch(&call.name, call.args.clone(), &response.content, idx);
            if outcome.should_exit {
                final_text = outcome.data.to_string();
                return Ok(AgentRunResult {
                    result: final_text,
                    turns: turn,
                    exit_reason: "EXITED".to_string(),
                });
            }
            if let Some(next) = outcome.next_prompt {
                if !next.is_empty() {
                    next_prompts.push(next);
                }
            } else {
                exit_reason = Some("CURRENT_TASK_DONE".to_string());
                final_text = response.content.clone();
                break;
            }
            if call.name != "no_tool" && !call.id.is_empty() {
                tool_results.push(ToolResult {
                    tool_use_id: call.id.clone(),
                    content: serde_json::to_string(&outcome.data)
                        .unwrap_or_else(|_| outcome.data.to_string()),
                });
            }
        }

        if let Some(reason) = exit_reason {
            handler.push_history(format!("[Agent] {}", summarize_response(&response)));
            return Ok(AgentRunResult {
                result: final_text,
                turns: turn,
                exit_reason: reason,
            });
        }
        let next_prompt = next_prompts.join("\n");
        handler.push_history(format!("[Agent] {}", summarize_response(&response)));
        messages.push(ChatMessage {
            role: "assistant".to_string(),
            content: response.content,
            tool_calls: response.tool_calls,
            tool_results: Vec::new(),
        });
        messages.push(ChatMessage {
            role: "user".to_string(),
            content: next_prompt,
            tool_calls: Vec::new(),
            tool_results,
        });
    }

    Ok(AgentRunResult {
        result: final_text,
        turns: max_turns,
        exit_reason: "MAX_TURNS_EXCEEDED".to_string(),
    })
}

fn summarize_response(response: &AssistantResponse) -> String {
    let trimmed = response.content.trim();
    if trimmed.is_empty() && !response.tool_calls.is_empty() {
        let names: Vec<&str> = response
            .tool_calls
            .iter()
            .map(|c| c.name.as_str())
            .collect();
        return format!("called tools: {}", names.join(", "));
    }
    smart_format(trimmed, 120)
}

fn smart_format(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let head: String = s.chars().take(max / 2).collect();
        let tail_len = max.saturating_sub(max / 2);
        let tail: String = s
            .chars()
            .rev()
            .take(tail_len)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();
        format!("{head} ... {tail}")
    }
}
