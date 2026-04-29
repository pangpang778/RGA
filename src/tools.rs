use anyhow::{Context, Result, anyhow};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::fs::{self, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::assets::RuntimePaths;

#[derive(Clone, Debug)]
pub struct ToolOutcome {
    pub data: Value,
    pub next_prompt: Option<String>,
    pub should_exit: bool,
}

#[derive(Debug)]
pub struct ToolDispatcher {
    pub paths: RuntimePaths,
    pub cwd: PathBuf,
    pub working: HashMap<String, String>,
    history: Vec<String>,
    pub max_turns: usize,
}

impl ToolDispatcher {
    pub fn new(paths: RuntimePaths, cwd: PathBuf) -> Self {
        Self {
            paths,
            cwd,
            working: HashMap::new(),
            history: Vec::new(),
            max_turns: 70,
        }
    }

    pub fn push_history(&mut self, entry: impl Into<String>) {
        self.history.push(entry.into());
    }

    pub fn dispatch(
        &mut self,
        tool_name: &str,
        mut args: Value,
        response_content: &str,
        index: usize,
    ) -> ToolOutcome {
        if let Value::Object(map) = &mut args {
            map.insert("_index".to_string(), json!(index));
        }
        let result = match tool_name {
            "code_run" => self.do_code_run(&args, response_content),
            "file_read" => self.do_file_read(&args),
            "file_patch" => self.do_file_patch(&args),
            "file_write" => self.do_file_write(&args, response_content),
            "web_scan" => self.do_web_scan(&args),
            "web_execute_js" => self.do_web_execute_js(&args, response_content),
            "update_working_checkpoint" => self.do_update_working_checkpoint(&args),
            "ask_user" => self.do_ask_user(&args),
            "start_long_term_update" => self.do_start_long_term_update(),
            "bad_json" => Ok(ToolOutcome {
                data: Value::Null,
                next_prompt: Some(
                    args.get("msg")
                        .and_then(Value::as_str)
                        .unwrap_or("bad_json")
                        .to_string(),
                ),
                should_exit: false,
            }),
            "no_tool" => Ok(ToolOutcome {
                data: json!({"content": response_content}),
                next_prompt: None,
                should_exit: false,
            }),
            other => Ok(ToolOutcome {
                data: json!({"status":"error","msg":format!("unknown tool: {other}")}),
                next_prompt: Some(format!("Unknown tool {other}")),
                should_exit: false,
            }),
        };
        result.unwrap_or_else(|e| ToolOutcome {
            data: json!({"status":"error","msg": e.to_string()}),
            next_prompt: Some("\n".to_string()),
            should_exit: false,
        })
    }

    fn normalize_path(path: PathBuf) -> PathBuf {
        path.components().fold(PathBuf::new(), |mut acc, c| {
            match c {
                Component::CurDir => {}
                Component::ParentDir => {
                    acc.pop();
                }
                other => acc.push(other.as_os_str()),
            }
            acc
        })
    }

    fn resolve_under(&self, root: &Path, raw: &str) -> Result<PathBuf> {
        if raw.trim().is_empty() {
            return Err(anyhow!("path is empty"));
        }
        let p = Path::new(raw);
        if p.is_absolute() && std::env::var("RGA_UNSAFE_ALLOW_ABSOLUTE").as_deref() != Ok("1") {
            return Err(anyhow!(
                "absolute paths are disabled; use a path inside the RGA sandbox"
            ));
        }
        let root =
            fs::canonicalize(root).with_context(|| format!("canonicalize {}", root.display()))?;
        let joined = if p.is_absolute() {
            p.to_path_buf()
        } else {
            root.join(p)
        };
        let candidate = Self::normalize_path(joined);
        if !candidate.starts_with(&root) {
            return Err(anyhow!("path escapes sandbox: {}", raw));
        }
        Ok(candidate)
    }

    fn resolve_read_path(&self, raw: &str) -> Result<PathBuf> {
        let roots = [&self.cwd, &self.paths.memory, &self.paths.assets];
        let mut last_err = None;
        for root in roots {
            match self.resolve_under(root, raw) {
                Ok(path) if path.exists() => return Ok(path),
                Ok(_) => {}
                Err(e) => last_err = Some(e),
            }
        }
        Err(last_err.unwrap_or_else(|| anyhow!("read path not found inside allowed roots: {raw}")))
    }

    fn resolve_write_path(&self, raw: &str) -> Result<PathBuf> {
        self.resolve_under(&self.cwd, raw)
    }

    fn anchor_prompt(&self, skip: bool) -> Option<String> {
        if skip {
            return Some("\n".to_string());
        }
        let mut prompt = String::from("\n### [WORKING MEMORY]\n<history>\n");
        let start = self.history.len().saturating_sub(40);
        prompt.push_str(&self.history[start..].join("\n"));
        prompt.push_str("\n</history>\n");
        if let Some(k) = self.working.get("key_info") {
            prompt.push_str("\n<key_info>");
            prompt.push_str(k);
            prompt.push_str("</key_info>");
        }
        Some(prompt)
    }

    fn do_code_run(&self, args: &Value, response_content: &str) -> Result<ToolOutcome> {
        let code_type = args
            .get("type")
            .or_else(|| args.get("code_type"))
            .and_then(Value::as_str)
            .unwrap_or("python");
        let code = args
            .get("code")
            .or_else(|| args.get("script"))
            .and_then(Value::as_str)
            .map(str::to_string)
            .or_else(|| extract_code_block(response_content, code_type))
            .unwrap_or_default();
        if code.trim().is_empty() {
            return Err(anyhow!(
                "code_run requires code/script or a matching fenced code block in the assistant reply"
            ));
        }
        let timeout = args.get("timeout").and_then(Value::as_u64).unwrap_or(60);
        let cwd = self.resolve_under(
            &self.cwd,
            args.get("cwd").and_then(Value::as_str).unwrap_or("."),
        )?;
        let data = run_code(&code, code_type, timeout, &cwd)?;
        Ok(ToolOutcome {
            data,
            next_prompt: self
                .anchor_prompt(args.get("_index").and_then(Value::as_u64).unwrap_or(0) > 0),
            should_exit: false,
        })
    }

    fn do_file_read(&self, args: &Value) -> Result<ToolOutcome> {
        let path =
            self.resolve_read_path(args.get("path").and_then(Value::as_str).unwrap_or(""))?;
        let start = args.get("start").and_then(Value::as_u64).unwrap_or(1) as usize;
        let count = args.get("count").and_then(Value::as_u64).unwrap_or(200) as usize;
        let keyword = args.get("keyword").and_then(Value::as_str);
        let show_linenos = args
            .get("show_linenos")
            .and_then(Value::as_bool)
            .unwrap_or(true);
        let content = file_read(&path, start, keyword, count, show_linenos)?;
        Ok(ToolOutcome {
            data: json!(content),
            next_prompt: self
                .anchor_prompt(args.get("_index").and_then(Value::as_u64).unwrap_or(0) > 0),
            should_exit: false,
        })
    }

    fn do_file_patch(&self, args: &Value) -> Result<ToolOutcome> {
        let path =
            self.resolve_write_path(args.get("path").and_then(Value::as_str).unwrap_or(""))?;
        let old = args
            .get("old_content")
            .and_then(Value::as_str)
            .unwrap_or("");
        let new = args
            .get("new_content")
            .and_then(Value::as_str)
            .unwrap_or("");
        let result = file_patch(&path, old, new)?;
        Ok(ToolOutcome {
            data: result,
            next_prompt: self
                .anchor_prompt(args.get("_index").and_then(Value::as_u64).unwrap_or(0) > 0),
            should_exit: false,
        })
    }

    fn do_file_write(&self, args: &Value, response_content: &str) -> Result<ToolOutcome> {
        let path =
            self.resolve_write_path(args.get("path").and_then(Value::as_str).unwrap_or(""))?;
        let mode = args
            .get("mode")
            .and_then(Value::as_str)
            .unwrap_or("overwrite");
        let content = args
            .get("content")
            .and_then(Value::as_str)
            .map(str::to_string)
            .or_else(|| extract_robust_content(response_content))
            .ok_or_else(|| anyhow!("file_write requires args.content or <file_content>...</file_content> in assistant content"))?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        match mode {
            "append" => {
                let mut f = OpenOptions::new().create(true).append(true).open(&path)?;
                f.write_all(content.as_bytes())?;
            }
            "prepend" => {
                let old = fs::read_to_string(&path).unwrap_or_default();
                fs::write(&path, format!("{content}{old}"))?;
            }
            _ => fs::write(&path, &content)?,
        }
        Ok(ToolOutcome {
            data: json!({"status":"success","written_bytes":content.len()}),
            next_prompt: self
                .anchor_prompt(args.get("_index").and_then(Value::as_u64).unwrap_or(0) > 0),
            should_exit: false,
        })
    }

    fn do_web_scan(&self, args: &Value) -> Result<ToolOutcome> {
        let tabs_only = args
            .get("tabs_only")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let text_only = args
            .get("text_only")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let switch_tab_id = args
            .get("switch_tab_id")
            .or_else(|| args.get("tab_id"))
            .and_then(Value::as_str);
        let bridge = BrowserBridge::default();
        let tabs = bridge.get_all_sessions()?;
        let data = if tabs_only {
            json!({"status":"success","metadata":{"tabs":tabs}})
        } else {
            let script = if text_only {
                "document.body ? document.body.innerText : document.documentElement.innerText"
            } else {
                "document.documentElement ? document.documentElement.outerHTML : ''"
            };
            let content = bridge.execute_js(script, switch_tab_id)?;
            json!({"status":"success","metadata":{"tabs":tabs,"active_tab":switch_tab_id},"content":content})
        };
        Ok(ToolOutcome {
            data,
            next_prompt: Some("\n".to_string()),
            should_exit: false,
        })
    }

    fn do_web_execute_js(&self, args: &Value, response_content: &str) -> Result<ToolOutcome> {
        let mut script = args
            .get("script")
            .and_then(Value::as_str)
            .map(str::to_string)
            .or_else(|| extract_code_block(response_content, "javascript"))
            .unwrap_or_default();
        if !script.trim().is_empty() {
            let maybe_path = self.resolve_read_path(script.trim());
            if let Ok(path) = maybe_path
                && path.is_file()
            {
                script = fs::read_to_string(path)?;
            }
        }
        if script.trim().is_empty() {
            return Err(anyhow!(
                "web_execute_js requires script or a javascript fenced code block"
            ));
        }
        let session_id = args
            .get("switch_tab_id")
            .or_else(|| args.get("tab_id"))
            .and_then(Value::as_str);
        let bridge = BrowserBridge::default();
        let data = bridge.execute_js(&script, session_id)?;
        Ok(ToolOutcome {
            data,
            next_prompt: self
                .anchor_prompt(args.get("_index").and_then(Value::as_u64).unwrap_or(0) > 0),
            should_exit: false,
        })
    }

    fn do_update_working_checkpoint(&mut self, args: &Value) -> Result<ToolOutcome> {
        if let Some(k) = args.get("key_info").and_then(Value::as_str) {
            self.working.insert("key_info".to_string(), k.to_string());
        }
        if let Some(s) = args.get("related_sop").and_then(Value::as_str) {
            self.working
                .insert("related_sop".to_string(), s.to_string());
        }
        Ok(ToolOutcome {
            data: json!({"result":"working key_info updated"}),
            next_prompt: self
                .anchor_prompt(args.get("_index").and_then(Value::as_u64).unwrap_or(0) > 0),
            should_exit: false,
        })
    }

    fn do_ask_user(&self, args: &Value) -> Result<ToolOutcome> {
        Ok(ToolOutcome {
            data: json!({"status":"INTERRUPT","intent":"HUMAN_INTERVENTION","data":{"question":args.get("question").and_then(Value::as_str).unwrap_or("Please provide input:"),"candidates":args.get("candidates").cloned().unwrap_or_else(|| json!([]))}}),
            next_prompt: None,
            should_exit: true,
        })
    }

    fn do_start_long_term_update(&self) -> Result<ToolOutcome> {
        let sop = self.paths.memory.join("memory_management_sop.md");
        let content = fs::read_to_string(sop).unwrap_or_else(|_| {
            "Memory Management SOP not found. Do not update memory.".to_string()
        });
        Ok(ToolOutcome {
            data: json!(content),
            next_prompt: Some(format!(
                "Extract durable verified lessons, then update memory minimally.\n\n{}",
                self.paths.global_memory()
            )),
            should_exit: false,
        })
    }
}

pub fn file_read(
    path: &Path,
    start: usize,
    keyword: Option<&str>,
    count: usize,
    show_linenos: bool,
) -> Result<String> {
    let text = fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    let lines: Vec<&str> = text.lines().collect();
    let mut begin = start.saturating_sub(1).min(lines.len());
    if let Some(kw) = keyword {
        let low = kw.to_ascii_lowercase();
        if let Some(pos) = lines
            .iter()
            .enumerate()
            .skip(begin)
            .find_map(|(i, l)| l.to_ascii_lowercase().contains(&low).then_some(i))
        {
            begin = pos.saturating_sub(count / 3);
        }
    }
    let end = (begin + count).min(lines.len());
    let mut out = String::new();
    if show_linenos {
        out.push_str(&format!("[FILE] {} lines", lines.len()));
        if end - begin < lines.len() {
            out.push_str(&format!(" | PARTIAL showing {}", end - begin));
        }
        out.push('\n');
    }
    for (i, line) in lines[begin..end].iter().enumerate() {
        if show_linenos {
            out.push_str(&format!("{}|{}\n", begin + i + 1, truncate(line, 8000)));
        } else {
            out.push_str(line);
            out.push('\n');
        }
    }
    Ok(out)
}

pub fn file_patch(path: &Path, old: &str, new: &str) -> Result<Value> {
    if old.is_empty() {
        return Ok(json!({"status":"error","msg":"old_content is empty"}));
    }
    let text = fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    let count = text.matches(old).count();
    if count == 0 {
        return Ok(json!({"status":"error","msg":"old_content not found"}));
    }
    if count > 1 {
        return Ok(
            json!({"status":"error","msg":format!("old_content matched {count} times; provide a more specific block")}),
        );
    }
    fs::write(path, text.replacen(old, new, 1))?;
    Ok(json!({"status":"success","msg":"file patched"}))
}

pub fn run_code(code: &str, code_type: &str, timeout_secs: u64, cwd: &Path) -> Result<Value> {
    fs::create_dir_all(cwd)?;
    let (program, args, temp_path) = match code_type {
        "python" | "py" => {
            let path = cwd.join(format!("rga-{}.py", unique_suffix()));
            fs::write(&path, code)?;
            (
                python_exe(),
                vec![
                    "-X".to_string(),
                    "utf8".to_string(),
                    "-u".to_string(),
                    path.to_string_lossy().to_string(),
                ],
                Some(path),
            )
        }
        "powershell" | "ps1" | "pwsh" => (
            "powershell".to_string(),
            vec![
                "-NoProfile".to_string(),
                "-NonInteractive".to_string(),
                "-Command".to_string(),
                code.to_string(),
            ],
            None,
        ),
        "bash" | "sh" | "shell" => {
            if cfg!(windows) {
                (
                    "powershell".to_string(),
                    vec![
                        "-NoProfile".to_string(),
                        "-NonInteractive".to_string(),
                        "-Command".to_string(),
                        code.to_string(),
                    ],
                    None,
                )
            } else {
                (
                    "bash".to_string(),
                    vec!["-c".to_string(), code.to_string()],
                    None,
                )
            }
        }
        other => {
            return Ok(json!({"status":"error","msg":format!("unsupported code type: {other}")}));
        }
    };

    let mut child = Command::new(&program)
        .args(&args)
        .current_dir(cwd)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .with_context(|| format!("spawn {program}"))?;

    let stdout = child.stdout.take().unwrap();
    let stderr = child.stderr.take().unwrap();
    let (tx, rx) = mpsc::channel::<String>();
    let tx2 = tx.clone();
    thread::spawn(move || {
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            let _ = tx.send(format!("{line}\n"));
        }
    });
    thread::spawn(move || {
        for line in BufReader::new(stderr).lines().map_while(Result::ok) {
            let _ = tx2.send(format!("{line}\n"));
        }
    });

    let start = Instant::now();
    let mut output = String::new();
    loop {
        while let Ok(chunk) = rx.try_recv() {
            output.push_str(&chunk);
        }
        if let Some(status) = child.try_wait()? {
            thread::sleep(Duration::from_millis(20));
            while let Ok(chunk) = rx.try_recv() {
                output.push_str(&chunk);
            }
            if let Some(path) = temp_path {
                let _ = fs::remove_file(path);
            }
            return Ok(
                json!({"status": if status.success() {"success"} else {"error"}, "stdout": truncate(&output, 10000), "exit_code": status.code()}),
            );
        }
        if start.elapsed() > Duration::from_secs(timeout_secs) {
            let _ = child.kill();
            break;
        }
        thread::sleep(Duration::from_millis(50));
    }
    let _ = child.wait();
    if let Some(path) = temp_path {
        let _ = fs::remove_file(path);
    }
    output.push_str("\n[Timeout Error] process killed");
    Ok(json!({"status":"error","stdout":truncate(&output, 10000),"exit_code":null}))
}

fn python_exe() -> String {
    std::env::var("PYTHON").unwrap_or_else(|_| {
        if cfg!(windows) {
            "python".to_string()
        } else {
            "python3".to_string()
        }
    })
}

fn unique_suffix() -> String {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos()
        .to_string()
}

fn extract_robust_content(text: &str) -> Option<String> {
    if let (Some(s), Some(e)) = (text.find("<file_content>"), text.rfind("</file_content>")) {
        let start = s + "<file_content>".len();
        if start <= e {
            return Some(text[start..e].trim().to_string());
        }
    }
    if let (Some(s), Some(e)) = (text.find("```"), text.rfind("```"))
        && s < e
    {
        let body_start = text[s..].find('\n').map(|n| s + n + 1).unwrap_or(s + 3);
        return Some(text[body_start..e].trim().to_string());
    }
    None
}

fn extract_code_block(text: &str, code_type: &str) -> Option<String> {
    let aliases: Vec<&str> = match code_type {
        "python" | "py" => vec!["python", "py"],
        "powershell" | "ps1" | "pwsh" => vec!["powershell", "ps1", "pwsh"],
        "bash" | "sh" | "shell" => vec!["bash", "sh", "shell"],
        "javascript" | "js" => vec!["javascript", "js"],
        other => vec![other],
    };
    let mut cursor = text;
    let mut last = None;
    while let Some(start) = cursor.find("```") {
        cursor = &cursor[start + 3..];
        let Some(line_end) = cursor.find('\n') else {
            break;
        };
        let lang = cursor[..line_end].trim().to_ascii_lowercase();
        cursor = &cursor[line_end + 1..];
        let Some(end) = cursor.find("```") else {
            break;
        };
        if aliases.iter().any(|a| lang == *a) {
            last = Some(cursor[..end].trim().to_string());
        }
        cursor = &cursor[end + 3..];
    }
    last
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        return s.to_string();
    }
    let half = max / 2;
    format!(
        "{}\n\n[omitted long output]\n\n{}",
        &s[..half],
        &s[s.len() - half..]
    )
}

#[derive(Default)]
struct BrowserBridge {
    host: String,
    port: u16,
}

impl BrowserBridge {
    fn ensure_enabled() -> Result<()> {
        if std::env::var("RGA_ENABLE_BROWSER_BRIDGE").as_deref() == Ok("1") {
            Ok(())
        } else {
            Err(anyhow!(
                "browser bridge is disabled; set RGA_ENABLE_BROWSER_BRIDGE=1 only for trusted local sessions"
            ))
        }
    }

    fn base(&self) -> String {
        let host = if self.host.is_empty() {
            "127.0.0.1"
        } else {
            &self.host
        };
        let port = if self.port == 0 { 18766 } else { self.port + 1 };
        format!("http://{host}:{port}/link")
    }

    fn get_all_sessions(&self) -> Result<Value> {
        Self::ensure_enabled()?;
        let client = reqwest::blocking::Client::new();
        let resp: Value = client
            .post(self.base())
            .json(&json!({"cmd":"get_all_sessions"}))
            .send()?
            .json()?;
        Ok(resp.get("r").cloned().unwrap_or(resp))
    }

    fn execute_js(&self, code: &str, session_id: Option<&str>) -> Result<Value> {
        Self::ensure_enabled()?;
        let client = reqwest::blocking::Client::new();
        let resp: Value = client
            .post(self.base())
            .json(&json!({"cmd":"execute_js","sessionId":session_id,"code":code,"timeout":"10"}))
            .send()?
            .json()?;
        Ok(resp.get("r").cloned().unwrap_or(resp))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn patch_requires_unique_match() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("a.txt");
        fs::write(&file, "one two one").unwrap();
        assert_eq!(file_patch(&file, "one", "x").unwrap()["status"], "error");
        assert_eq!(file_patch(&file, "two", "2").unwrap()["status"], "success");
        assert_eq!(fs::read_to_string(file).unwrap(), "one 2 one");
    }

    #[test]
    fn reads_with_line_numbers() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("a.txt");
        fs::write(&file, "a\nb\nc\n").unwrap();
        let out = file_read(&file, 2, None, 1, true).unwrap();
        assert!(out.contains("2|b"));
    }

    #[test]
    fn extracts_file_content_tag() {
        assert_eq!(
            extract_robust_content("x<file_content>abc</file_content>y").unwrap(),
            "abc"
        );
    }
}
