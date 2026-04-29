use anyhow::Result;
use serde_json::Value;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SessionSummary {
    pub path: PathBuf,
    pub mtime_epoch: u64,
    pub preview: String,
    pub rounds: usize,
}

pub fn log_dir(temp: &Path) -> PathBuf {
    temp.join("model_responses")
}

pub fn current_log_path(temp: &Path) -> PathBuf {
    log_dir(temp).join(format!("model_responses_{}.txt", std::process::id()))
}

pub fn append_turn(temp: &Path, prompt: &Value, response: &Value) -> Result<()> {
    let dir = log_dir(temp);
    fs::create_dir_all(&dir)?;
    let path = current_log_path(temp);
    let mut file = OpenOptions::new().create(true).append(true).open(&path)?;
    writeln!(file, "=== Prompt ===")?;
    writeln!(file, "{}", serde_json::to_string_pretty(prompt)?)?;
    writeln!(file, "=== Response ===")?;
    writeln!(file, "{}", serde_json::to_string_pretty(response)?)?;
    Ok(())
}

pub fn snapshot_current(temp: &Path) -> Result<Option<PathBuf>> {
    let path = current_log_path(temp);
    if !path.is_file() {
        return Ok(None);
    }
    let content = fs::read_to_string(&path)?;
    if parse_pairs(&content).is_empty() {
        return Ok(None);
    }
    let stamp = chrono::Local::now().format("%Y%m%d_%H%M%S");
    let ns = SystemTime::now().duration_since(UNIX_EPOCH)?.subsec_nanos();
    let snapshot = log_dir(temp).join(format!(
        "model_responses_snapshot_{}_{}_{:09}.txt",
        std::process::id(),
        stamp,
        ns
    ));
    fs::write(&snapshot, content)?;
    fs::write(&path, "")?;
    Ok(Some(snapshot))
}

pub fn list_sessions(temp: &Path, exclude_current: bool) -> Result<Vec<SessionSummary>> {
    let dir = log_dir(temp);
    if !dir.is_dir() {
        return Ok(Vec::new());
    }
    let current = current_log_path(temp);
    let mut out = Vec::new();
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if !path.is_file() || path.extension().and_then(|s| s.to_str()) != Some("txt") {
            continue;
        }
        if exclude_current && path == current {
            continue;
        }
        let content = fs::read_to_string(&path).unwrap_or_default();
        let pairs = parse_pairs(&content);
        if pairs.is_empty() {
            continue;
        }
        let meta = entry.metadata()?;
        let mtime_epoch = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .map(|d| d.as_secs())
            .unwrap_or_default();
        out.push(SessionSummary {
            path,
            mtime_epoch,
            preview: preview_text(&pairs),
            rounds: pairs.len(),
        });
    }
    out.sort_by_key(|s| std::cmp::Reverse(s.mtime_epoch));
    Ok(out)
}

pub fn format_list(sessions: &[SessionSummary], limit: usize) -> String {
    if sessions.is_empty() {
        return "No recoverable sessions".to_string();
    }
    let mut lines = vec![
        "Recoverable sessions (use /continue N):".to_string(),
        String::new(),
    ];
    for (idx, s) in sessions.iter().take(limit).enumerate() {
        lines.push(format!(
            "{}. {} rounds | {} | {}",
            idx + 1,
            s.rounds,
            rel_time(s.mtime_epoch),
            s.preview
                .replace('\n', " ")
                .chars()
                .take(80)
                .collect::<String>()
        ));
    }
    lines.join("\n")
}

pub fn restore_preview(temp: &Path, index_1based: usize) -> Result<String> {
    let sessions = list_sessions(temp, true)?;
    let Some(session) = sessions.get(index_1based.saturating_sub(1)) else {
        return Ok(format!("Index out of range (valid 1-{})", sessions.len()));
    };
    let _ = snapshot_current(temp)?;
    Ok(format!(
        "Selected historical session: {} ({} rounds). RGA snapshotted the current log; exact native backend replay is represented by the session log artifact.",
        session
            .path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("session"),
        session.rounds
    ))
}

fn parse_pairs(content: &str) -> Vec<(String, String)> {
    let mut pairs = Vec::new();
    let mut current_prompt: Option<String> = None;
    let mut label: Option<&str> = None;
    let mut buf = String::new();
    for line in content.lines() {
        match line {
            "=== Prompt ===" | "=== Response ===" => {
                if let Some(prev) = label.take() {
                    if prev == "Prompt" {
                        current_prompt = Some(buf.trim().to_string());
                    } else if let Some(prompt) = current_prompt.take() {
                        pairs.push((prompt, buf.trim().to_string()));
                    }
                    buf.clear();
                }
                label = Some(if line.contains("Prompt") {
                    "Prompt"
                } else {
                    "Response"
                });
            }
            _ => {
                buf.push_str(line);
                buf.push('\n');
            }
        }
    }
    if let Some("Response") = label
        && let Some(prompt) = current_prompt.take()
    {
        pairs.push((prompt, buf.trim().to_string()));
    }
    pairs
}

fn preview_text(pairs: &[(String, String)]) -> String {
    for (prompt, _) in pairs {
        if let Ok(v) = serde_json::from_str::<Value>(prompt)
            && let Some(content) = v.get("content").and_then(Value::as_str)
            && !content.contains("WORKING MEMORY")
            && !content.trim().is_empty()
        {
            return content.trim().to_string();
        }
    }
    pairs
        .first()
        .map(|(p, _)| p.lines().find(|l| !l.trim().is_empty()).unwrap_or(""))
        .unwrap_or("")
        .to_string()
}

fn rel_time(epoch: u64) -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(epoch);
    let d = now.saturating_sub(epoch);
    if d < 60 {
        format!("{}s ago", d)
    } else if d < 3600 {
        format!("{}m ago", d / 60)
    } else if d < 86400 {
        format!("{}h ago", d / 3600)
    } else {
        format!("{}d ago", d / 86400)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn logs_and_lists_sessions() {
        let tmp = tempfile::tempdir().unwrap();
        append_turn(
            tmp.path(),
            &json!({"role":"user","content":"hello"}),
            &json!({"content":"world"}),
        )
        .unwrap();
        let list = list_sessions(tmp.path(), false).unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].rounds, 1);
        assert_eq!(list[0].preview, "hello");
    }
}
