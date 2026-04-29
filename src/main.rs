use anyhow::{Context, Result, anyhow};
use clap::Parser;
use rga::agent_loop::agent_runner_loop;
use rga::assets::RuntimePaths;
use rga::config::LlmConfig;
use rga::frontends::{self, FrontendKind};
use rga::llm::AnyLlmClient;
use rga::scheduler;
use rga::session_log;
use rga::tools::ToolDispatcher;
use std::collections::HashMap;
use std::fs;
use std::io::{self, Read, Write};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

#[derive(Parser, Debug, Clone)]
#[command(name = "rga", version, about = "Rust GenericAgent runtime")]
struct Args {
    /// File-IO task directory name under temp/ (compatible with GenericAgent --task)
    #[arg(long)]
    task: Option<String>,

    /// Prompt for one-shot execution. If omitted, starts REPL/task-file/reflect mode.
    #[arg(long)]
    input: Option<String>,

    /// Provider: mock, openai, anthropic. Defaults to env inference.
    #[arg(long)]
    provider: Option<String>,

    /// Accepted for compatibility; provider selection currently uses --provider/env.
    #[arg(long = "llm-no", alias = "llm_no", default_value_t = 0)]
    llm_no: usize,

    /// Spawn this RGA invocation in the background and print its PID.
    #[arg(long)]
    bg: bool,

    /// Reflect mode script. scheduler.py is handled natively; other Python scripts use compatibility subprocess probing.
    #[arg(long)]
    reflect: Option<String>,

    /// Start the default local GUI and open it in the browser.
    #[arg(long)]
    gui: bool,

    /// Open the local GUI URL in the default browser when starting a frontend.
    #[arg(long)]
    open: bool,

    /// Start a rewritten frontend adapter: streamlit/web, qt, telegram, feishu, wechat, wecom, dingtalk, qq, pet.
    #[arg(long)]
    frontend: Option<String>,

    /// Port for local frontend/web adapter.
    #[arg(long, default_value_t = 18501)]
    port: u16,

    /// Print tool calls and turn progress.
    #[arg(long)]
    verbose: bool,

    /// Maximum agent turns per user/reflect/task round.
    #[arg(long, default_value_t = 70)]
    max_turns: usize,
}

fn main() -> Result<()> {
    let args = Args::parse();
    let paths = RuntimePaths::discover()?;
    if args.bg {
        return spawn_background(&paths, &args);
    }
    if args.llm_no != 0 {
        eprintln!(
            "[WARN] --llm-no is accepted for compatibility but RGA selects providers through --provider/env."
        );
    }
    if args.gui {
        return rga::gui::run_gui(paths);
    }
    if let Some(frontend) = &args.frontend {
        return frontend_loop(&paths, frontend, args.port, args.open);
    }
    let task_dir = paths.task_dir(args.task.as_deref())?;
    let tools_schema = paths.load_tools_schema()?;
    let cfg = LlmConfig::from_env(args.provider.as_deref());
    let mut client = AnyLlmClient::from_config(cfg)?;
    let mut handler = ToolDispatcher::new(paths.clone(), paths.temp.clone());
    handler.max_turns = args.max_turns;

    if let Some(reflect) = &args.reflect
        && args.input.is_none()
        && args.task.is_none()
    {
        return reflect_loop(
            &paths,
            &mut client,
            &mut handler,
            tools_schema,
            reflect,
            args.max_turns,
            args.verbose,
        );
    }

    if args.task.is_some() {
        return task_file_loop(
            &paths,
            &task_dir,
            &mut client,
            &mut handler,
            tools_schema,
            args.input,
            args.max_turns,
            args.verbose,
        );
    }

    if let Some(input) = args.input {
        run_once(
            &paths,
            &mut client,
            &mut handler,
            tools_schema,
            input,
            RunOptions {
                max_turns: args.max_turns,
                verbose: args.verbose,
                output_path: None,
            },
        )?;
    } else {
        repl(
            &paths,
            &mut client,
            &mut handler,
            tools_schema,
            args.max_turns,
            args.verbose,
        )?;
    }
    Ok(())
}

fn spawn_background(paths: &RuntimePaths, args: &Args) -> Result<()> {
    let task_name = args.task.as_deref().unwrap_or("bg");
    let task_dir = paths.task_dir(Some(task_name))?;
    let exe = std::env::current_exe()?;
    let mut cmd = Command::new(exe);
    if let Some(task) = &args.task {
        cmd.args(["--task", task]);
    }
    if let Some(input) = &args.input {
        cmd.args(["--input", input]);
    }
    if let Some(provider) = &args.provider {
        cmd.args(["--provider", provider]);
    }
    if let Some(reflect) = &args.reflect {
        cmd.args(["--reflect", reflect]);
    }
    if args.verbose {
        cmd.arg("--verbose");
    }
    cmd.args(["--max-turns", &args.max_turns.to_string()]);
    let stdout = fs::File::create(task_dir.join("stdout.log"))?;
    let stderr = fs::File::create(task_dir.join("stderr.log"))?;
    let child = cmd
        .current_dir(&paths.root)
        .stdin(Stdio::null())
        .stdout(stdout)
        .stderr(stderr)
        .spawn()
        .context("spawn background RGA")?;
    println!("{}", child.id());
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn task_file_loop(
    paths: &RuntimePaths,
    task_dir: &std::path::Path,
    client: &mut AnyLlmClient,
    handler: &mut ToolDispatcher,
    tools_schema: serde_json::Value,
    input: Option<String>,
    max_turns: usize,
    verbose: bool,
) -> Result<()> {
    fs::create_dir_all(task_dir)?;
    if let Some(input) = input {
        for entry in fs::read_dir(task_dir)? {
            let path = entry?.path();
            if path
                .file_name()
                .and_then(|s| s.to_str())
                .is_some_and(|n| n.starts_with("output") && n.ends_with(".txt"))
            {
                let _ = fs::remove_file(path);
            }
        }
        fs::write(task_dir.join("input.txt"), &input)?;
    }
    let mut raw = fs::read_to_string(task_dir.join("input.txt"))
        .context("task mode requires input.txt or --input")?;
    let mut round: usize = 0;
    loop {
        let output_name = if round == 0 {
            "output.txt".to_string()
        } else {
            format!("output{round}.txt")
        };
        run_once(
            paths,
            client,
            handler,
            tools_schema.clone(),
            raw,
            RunOptions {
                max_turns,
                verbose,
                output_path: Some(task_dir.join(output_name)),
            },
        )?;
        let _ = fs::remove_file(task_dir.join("_stop"));
        match wait_and_consume(
            task_dir.join("reply.txt"),
            Duration::from_secs(reply_timeout_secs()),
        )? {
            Some(reply) => {
                raw = reply;
                round += 1;
            }
            None => break,
        }
    }
    Ok(())
}

struct RunOptions {
    max_turns: usize,
    verbose: bool,
    output_path: Option<std::path::PathBuf>,
}

fn run_once(
    paths: &RuntimePaths,
    client: &mut AnyLlmClient,
    handler: &mut ToolDispatcher,
    tools_schema: serde_json::Value,
    input: String,
    options: RunOptions,
) -> Result<String> {
    let result = agent_runner_loop(
        client,
        paths.system_prompt()?,
        input,
        handler,
        tools_schema,
        options.max_turns,
        options.verbose,
        None,
    )?;
    println!(
        "\n[Exit] {} after {} turn(s)",
        result.exit_reason, result.turns
    );
    if let Some(path) = options.output_path {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(path, format!("{}\n[ROUND END]\n", result.result))?;
    }
    Ok(result.result)
}

fn repl(
    paths: &RuntimePaths,
    client: &mut AnyLlmClient,
    handler: &mut ToolDispatcher,
    tools_schema: serde_json::Value,
    max_turns: usize,
    verbose: bool,
) -> Result<()> {
    println!("RGA interactive mode. Type /exit to quit. Use /continue to list logged sessions.");
    loop {
        print!("> ");
        io::stdout().flush()?;
        let mut line = String::new();
        if io::stdin().read_line(&mut line)? == 0 {
            break;
        }
        let line = line.trim_end().to_string();
        if line == "/exit" || line == "/quit" {
            break;
        }
        if line.trim().is_empty() {
            continue;
        }
        if handle_slash(paths, &line)? {
            continue;
        }
        run_once(
            paths,
            client,
            handler,
            tools_schema.clone(),
            line,
            RunOptions {
                max_turns,
                verbose,
                output_path: None,
            },
        )?;
    }
    Ok(())
}

fn handle_slash(paths: &RuntimePaths, line: &str) -> Result<bool> {
    if line == "/continue" {
        let sessions = session_log::list_sessions(&paths.temp, true)?;
        println!("{}", session_log::format_list(&sessions, 20));
        return Ok(true);
    }
    if let Some(rest) = line.strip_prefix("/continue ") {
        let idx = rest.trim().parse::<usize>().unwrap_or(0);
        println!("{}", session_log::restore_preview(&paths.temp, idx)?);
        return Ok(true);
    }
    if line == "/new" || line == "/reset" {
        let _ = session_log::snapshot_current(&paths.temp)?;
        println!("Started a new conversation; current model_responses log was snapshotted.");
        return Ok(true);
    }
    Ok(false)
}

#[allow(clippy::too_many_arguments)]
fn reflect_loop(
    paths: &RuntimePaths,
    client: &mut AnyLlmClient,
    handler: &mut ToolDispatcher,
    tools_schema: serde_json::Value,
    reflect: &str,
    max_turns: usize,
    verbose: bool,
) -> Result<()> {
    println!("[Reflect] loaded {reflect}");
    loop {
        let task = if reflect.replace('\\', "/").ends_with("reflect/scheduler.py")
            || reflect.ends_with("scheduler.py")
        {
            scheduler::check_scheduled_tasks(&paths.root)?.map(|t| t.prompt)
        } else {
            python_reflect_check(reflect)?
        };
        if let Some(task) = task {
            println!(
                "[Reflect] triggered: {}",
                task.chars().take(80).collect::<String>()
            );
            let result = run_once(
                paths,
                client,
                handler,
                tools_schema.clone(),
                task,
                RunOptions {
                    max_turns,
                    verbose,
                    output_path: None,
                },
            )?;
            let log_dir = paths.temp.join("reflect_logs");
            fs::create_dir_all(&log_dir)?;
            let script_name = std::path::Path::new(reflect)
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("reflect");
            let log = log_dir.join(format!(
                "{}_{}.log",
                script_name,
                chrono::Local::now().format("%Y-%m-%d")
            ));
            fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(log)?
                .write_all(
                    format!(
                        "[{}]\n{}\n\n",
                        chrono::Local::now().format("%m-%d %H:%M"),
                        result
                    )
                    .as_bytes(),
                )?;
            if std::env::var("RGA_REFLECT_ONCE").as_deref() == Ok("1") {
                break;
            }
        }
        thread::sleep(Duration::from_secs(reflect_interval(reflect)));
    }
    Ok(())
}

fn python_reflect_check(script: &str) -> Result<Option<String>> {
    let code = r#"
import importlib.util, json, sys
path=sys.argv[1]
spec=importlib.util.spec_from_file_location('reflect_script', path)
mod=importlib.util.module_from_spec(spec)
spec.loader.exec_module(mod)
r=mod.check()
if r is not None:
    print(json.dumps(str(r), ensure_ascii=False))
"#;
    let out = Command::new("python")
        .args(["-c", code, script])
        .output()
        .context("run Python reflect check")?;
    if !out.status.success() {
        return Err(anyhow!(String::from_utf8_lossy(&out.stderr).to_string()));
    }
    let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if s.is_empty() {
        Ok(None)
    } else {
        Ok(Some(serde_json::from_str::<String>(&s).unwrap_or(s)))
    }
}

fn reflect_interval(_script: &str) -> u64 {
    std::env::var("RGA_REFLECT_INTERVAL")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(5)
}

fn reply_timeout_secs() -> u64 {
    std::env::var("RGA_REPLY_TIMEOUT_SECS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(600)
}

fn wait_and_consume(path: std::path::PathBuf, timeout: Duration) -> Result<Option<String>> {
    let start = Instant::now();
    while start.elapsed() < timeout {
        if path.is_file() {
            let content = fs::read_to_string(&path)?;
            let _ = fs::remove_file(&path);
            return Ok(Some(content));
        }
        thread::sleep(Duration::from_secs(2));
    }
    Ok(None)
}

fn frontend_loop(
    paths: &RuntimePaths,
    frontend: &str,
    port: u16,
    open_browser: bool,
) -> Result<()> {
    let Some(kind) = FrontendKind::from_name(frontend) else {
        return Err(anyhow!("unknown frontend: {frontend}"));
    };
    match kind {
        FrontendKind::Streamlit | FrontendKind::Qt | FrontendKind::DesktopPet => {
            local_web_frontend(paths, frontend, port, open_browser)
        }
        _ => {
            println!(
                "RGA {frontend} adapter is configured. Native SDK polling is intentionally replaced by the common file/session runtime; use the original platform credentials through env and connect via webhook bridge."
            );
            println!("{}", frontends::build_help_text());
            Ok(())
        }
    }
}

fn open_default_browser(url: &str) {
    #[cfg(target_os = "windows")]
    {
        let _ = Command::new("cmd").args(["/C", "start", "", url]).spawn();
    }
    #[cfg(target_os = "macos")]
    {
        let _ = Command::new("open").arg(url).spawn();
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        let _ = Command::new("xdg-open").arg(url).spawn();
    }
}

fn local_web_frontend(
    paths: &RuntimePaths,
    frontend: &str,
    port: u16,
    open_browser: bool,
) -> Result<()> {
    let listener = std::net::TcpListener::bind(("127.0.0.1", port))?;
    let url = format!("http://127.0.0.1:{port}");
    println!("RGA {frontend} GUI listening on {url}");
    if open_browser {
        open_default_browser(&url);
    }
    for stream in listener.incoming() {
        let mut stream = stream?;
        let request = read_http_request(&mut stream)?;
        let body = if request.method == "POST" && request.path == "/chat" {
            match handle_gui_chat(paths, frontend, &request.body) {
                Ok(answer) => render_gui(paths, frontend, Some(&answer), None),
                Err(e) => render_gui(paths, frontend, None, Some(&e.to_string())),
            }
        } else if request.method == "POST" && request.path == "/new" {
            let _ = save_gui_history(paths, &[]);
            render_gui(paths, frontend, None, None)
        } else {
            render_gui(paths, frontend, None, None)
        };
        write_http_response(&mut stream, &body)?;
    }
    Ok(())
}

#[derive(Debug)]
struct HttpRequest {
    method: String,
    path: String,
    body: String,
}

fn read_http_request(stream: &mut std::net::TcpStream) -> Result<HttpRequest> {
    stream.set_read_timeout(Some(Duration::from_secs(5)))?;
    let mut buf = Vec::new();
    let mut tmp = [0u8; 4096];
    loop {
        let n = stream.read(&mut tmp)?;
        if n == 0 {
            break;
        }
        buf.extend_from_slice(&tmp[..n]);
        if buf.windows(4).any(|w| w == b"\r\n\r\n") {
            break;
        }
        if buf.len() > 1024 * 1024 {
            return Err(anyhow!("request too large"));
        }
    }
    let header_end = buf
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .map(|p| p + 4)
        .ok_or_else(|| anyhow!("bad HTTP request"))?;
    let head = String::from_utf8_lossy(&buf[..header_end]);
    let mut lines = head.lines();
    let first = lines.next().unwrap_or_default();
    let mut parts = first.split_whitespace();
    let method = parts.next().unwrap_or("GET").to_string();
    let path = parts.next().unwrap_or("/").to_string();
    let content_len = lines
        .filter_map(|l| l.split_once(':'))
        .find(|(k, _)| k.eq_ignore_ascii_case("content-length"))
        .and_then(|(_, v)| v.trim().parse::<usize>().ok())
        .unwrap_or(0);
    while buf.len() < header_end + content_len {
        let n = stream.read(&mut tmp)?;
        if n == 0 {
            break;
        }
        buf.extend_from_slice(&tmp[..n]);
    }
    let body = String::from_utf8_lossy(&buf[header_end..buf.len().min(header_end + content_len)])
        .to_string();
    Ok(HttpRequest { method, path, body })
}

fn write_http_response(stream: &mut std::net::TcpStream, body: &str) -> Result<()> {
    let resp = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        body.len(),
        body
    );
    stream.write_all(resp.as_bytes())?;
    Ok(())
}

fn handle_gui_chat(paths: &RuntimePaths, _frontend: &str, raw_body: &str) -> Result<String> {
    let form = parse_form(raw_body);
    let prompt = form
        .get("prompt")
        .map(String::as_str)
        .unwrap_or("")
        .trim()
        .to_string();
    if prompt.is_empty() {
        return Err(anyhow!("请输入 prompt"));
    }
    let mut history = load_gui_history(paths).unwrap_or_default();
    let provider = form.get("provider").map(String::as_str).unwrap_or("mock");
    let cfg = match provider {
        "openai" => LlmConfig::openai_with(
            non_empty(form.get("api_key")).or_else(|| LlmConfig::openai().api_key),
            non_empty(form.get("base_url"))
                .unwrap_or_else(|| "https://api.minimaxi.com/v1".to_string()),
            non_empty(form.get("model")).unwrap_or_else(|| "MiniMax-M2.7".to_string()),
        ),
        "anthropic" => LlmConfig::anthropic_with(
            non_empty(form.get("api_key")).or_else(|| LlmConfig::anthropic().api_key),
            non_empty(form.get("base_url"))
                .unwrap_or_else(|| "https://api.anthropic.com".to_string()),
            non_empty(form.get("model")).unwrap_or_else(|| "claude-sonnet-4-6".to_string()),
        ),
        _ => LlmConfig::mock(),
    };
    let mut client = AnyLlmClient::from_config(cfg)?;
    let mut handler = ToolDispatcher::new(paths.clone(), paths.temp.clone());
    let result = agent_runner_loop(
        &mut client,
        paths.system_prompt()?,
        prompt.clone(),
        &mut handler,
        paths.load_tools_schema()?,
        8,
        false,
        None,
    )?;
    history.push(("user".to_string(), prompt));
    history.push(("assistant".to_string(), result.result.clone()));
    let _ = save_gui_history(paths, &history);
    Ok(result.result)
}

fn non_empty(v: Option<&String>) -> Option<String> {
    v.map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

fn parse_form(body: &str) -> HashMap<String, String> {
    body.split('&')
        .filter_map(|pair| {
            let (k, v) = pair.split_once('=')?;
            Some((url_decode(k), url_decode(v)))
        })
        .collect()
}

fn url_decode(s: &str) -> String {
    let mut out = Vec::new();
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            b'%' if i + 2 < bytes.len() => {
                if let Ok(hex) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                    out.push(hex);
                    i += 3;
                } else {
                    out.push(bytes[i]);
                    i += 1;
                }
            }
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).to_string()
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn gui_history_path(paths: &RuntimePaths) -> std::path::PathBuf {
    paths.temp.join("gui_history.json")
}

fn load_gui_history(paths: &RuntimePaths) -> Result<Vec<(String, String)>> {
    let path = gui_history_path(paths);
    if !path.is_file() {
        return Ok(Vec::new());
    }
    Ok(serde_json::from_str(&fs::read_to_string(path)?)?)
}

fn save_gui_history(paths: &RuntimePaths, history: &[(String, String)]) -> Result<()> {
    fs::create_dir_all(&paths.temp)?;
    fs::write(
        gui_history_path(paths),
        serde_json::to_string_pretty(history)?,
    )?;
    Ok(())
}

fn render_gui(
    paths: &RuntimePaths,
    frontend: &str,
    answer: Option<&str>,
    error: Option<&str>,
) -> String {
    let history = load_gui_history(paths).unwrap_or_default();
    let mut bubbles = String::new();
    if history.is_empty() {
        bubbles.push_str("<div class=\"empty-state\"><div class=\"empty-icon\">&#129302;</div><h3>欢迎使用 RGA Cowork</h3><p>请在左侧面板配置 Provider 和 API Key ，然后开始对话</p></div>");
    }
    for (role, content) in &history {
        let label = if role == "user" { "你" } else { "RGA" };
        let cls = if role == "user" { "user" } else { "assistant" };
        bubbles.push_str(&format!(
            "<div class=\"msg {}\"><div class=\"avatar\">{}</div><div class=\"bubble\"><pre>{}</pre></div></div>",
            cls,
            html_escape(label),
            html_escape(content)
        ));
    }
    let answer_html = answer
        .filter(|_| history.is_empty())
        .map(|a| {
            format!(
                "<section class=\"answer\"><h3>回答</h3><pre>{}</pre></section>",
                html_escape(a)
            )
        })
        .unwrap_or_default();
    let error_html = error
        .map(|e| {
            format!(
                "<section class=\"error\"><b>错误：</b>{}</section>",
                html_escape(e)
            )
        })
        .unwrap_or_default();
    let commands_html = frontends::build_help_text();
    format!(
        r#"<!doctype html>
<html lang=\"zh-CN\">
<head>
<meta charset=\"utf-8\">
<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">
<title>RGA Cowork</title>
<style>
*{{box-sizing:border-box;margin:0;padding:0}}
:root{{
  --bg:#0f172a;--bg2:#1e293b;--bg3:#334155;
  --fg:#e2e8f0;--fg2:#94a3b8;--fg3:#64748b;
  --accent:#38bdf8;--accent2:#818cf8;--accent-bg:#0c4a6e;
  --user-bg:#075985;--user-border:#0ea5e9;
  --asst-bg:#1e1b4b;--asst-border:#818cf8;
  --err-bg:#7f1d1d;--err-border:#ef4444;
  --radius:12px;
}}
html,body{{height:100%;overflow:hidden}}
body{{font-family:system-ui,-apple-system,"Microsoft YaHei",sans-serif;background:var(--bg);color:var(--fg)}}
.app{{display:grid;grid-template-columns:300px 1fr;height:100vh}}

/* Sidebar */
.side{{background:var(--bg2);border-right:1px solid var(--bg3);display:flex;flex-direction:column;overflow:hidden}}
.side-header{{padding:20px;border-bottom:1px solid var(--bg3)}}
.brand{{font-size:20px;font-weight:800;background:linear-gradient(135deg,var(--accent),var(--accent2));-webkit-background-clip:text;-webkit-text-fill-color:transparent}}
.brand-sub{{font-size:12px;color:var(--fg3);margin-top:4px}}
.side-body{{flex:1;overflow-y:auto;padding:16px}}
.side-footer{{padding:12px 16px;border-top:1px solid var(--bg3)}}

/* Form controls */
label{{display:block;margin-top:14px;color:var(--fg2);font-size:13px;font-weight:500}}
select,input{{width:100%;padding:9px 12px;border-radius:8px;border:1px solid var(--bg3);background:var(--bg);color:var(--fg);font-size:14px;margin-top:4px;outline:none;transition:border-color .2s}}
select:focus,input:focus{{border-color:var(--accent)}}
input[type=password]{{font-family:monospace}}

/* Buttons */
.btn{{display:inline-flex;align-items:center;justify-content:center;gap:6px;padding:10px 18px;border:0;border-radius:8px;font-weight:600;font-size:14px;cursor:pointer;transition:all .15s;width:100%;margin-top:14px}}
.btn-primary{{background:var(--accent);color:#082f49}}
.btn-primary:hover{{background:#7dd3fc}}
.btn-secondary{{background:var(--bg3);color:var(--fg)}}
.btn-secondary:hover{{background:#475569}}
.btn-danger{{background:#991b1b;color:#fca5a5}}
.btn-danger:hover{{background:#b91c1c}}

/* Help card */
.help-card{{background:var(--bg);border:1px solid var(--bg3);border-radius:var(--radius);padding:12px;margin-top:16px;font-size:12px;color:var(--fg3);white-space:pre-wrap;line-height:1.6;max-height:160px;overflow-y:auto}}
.help-card b{{color:var(--accent)}}

/* Main area */
.main{{display:flex;flex-direction:column;height:100vh;overflow:hidden}}
.topbar{{padding:16px 24px;border-bottom:1px solid var(--bg3);background:var(--bg2);display:flex;justify-content:space-between;align-items:center;flex-shrink:0}}
.topbar-title{{font-weight:700;font-size:16px}}
.topbar-sub{{font-size:12px;color:var(--fg3);margin-top:2px}}

/* Chat area */
.chat{{flex:1;overflow-y:auto;padding:24px;scroll-behavior:smooth}}

/* Messages */
.msg{{display:flex;gap:12px;margin:20px 0;align-items:flex-start;animation:fadeIn .3s ease}}
.msg.user{{flex-direction:row-reverse}}
.avatar{{width:36px;height:36px;border-radius:50%;display:flex;align-items:center;justify-content:center;font-weight:800;font-size:14px;flex-shrink:0}}
.msg.user .avatar{{background:var(--accent);color:#082f49}}
.msg.assistant .avatar{{background:var(--accent2);color:#1e1b4b}}
.bubble{{max-width:min(720px,80%)}}
.bubble pre{{margin:0;white-space:pre-wrap;word-break:break-word;padding:14px 18px;border-radius:var(--radius);line-height:1.65;font-size:14px;font-family:ui-monospace,SFMono-Regular,"SF Mono",Consolas,"Liberation Mono",monospace}}
.msg.user .bubble pre{{background:var(--user-bg);border:1px solid var(--user-border)}}
.msg.assistant .bubble pre{{background:var(--asst-bg);border:1px solid var(--asst-border)}}

/* Empty state */
.empty-state{{display:flex;flex-direction:column;align-items:center;justify-content:center;height:100%;color:var(--fg3);text-align:center;gap:12px}}
.empty-state .empty-icon{{font-size:48px;opacity:.6}}
.empty-state h3{{color:var(--fg2);font-size:20px}}
.empty-state p{{font-size:14px;max-width:400px}}

/* Error */
.error{{background:var(--err-bg);border:1px solid var(--err-border);padding:14px;border-radius:var(--radius);margin:16px 24px;font-size:14px}}
.error b{{color:#fca5a5}}

/* Answer */
.answer{{margin:16px 24px;padding:16px;background:var(--asst-bg);border:1px solid var(--asst-border);border-radius:var(--radius)}}
.answer h3{{margin-bottom:8px;color:var(--accent2)}}
.answer pre{{white-space:pre-wrap;line-height:1.65}}

/* Composer */
.composer{{border-top:1px solid var(--bg3);padding:16px 24px;background:var(--bg2);display:flex;gap:12px;flex-shrink:0;align-items:flex-end}}
.composer textarea{{flex:1;min-height:44px;max-height:200px;padding:12px 16px;border-radius:var(--radius);border:1px solid var(--bg3);background:var(--bg);color:var(--fg);font-size:14px;font-family:inherit;resize:vertical;outline:none;transition:border-color .2s}}
.composer textarea:focus{{border-color:var(--accent)}}
.composer textarea::placeholder{{color:var(--fg3)}}
.composer .btn{{width:auto;margin-top:0;padding:12px 24px;flex-shrink:0}}

/* Responsive */
@media(max-width:768px){{
  .app{{grid-template-columns:1fr}}
  .side{{display:none}}
}}

/* Scrollbar */
::-webkit-scrollbar{{width:6px}}
::-webkit-scrollbar-track{{background:transparent}}
::-webkit-scrollbar-thumb{{background:var(--bg3);border-radius:3px}}
::-webkit-scrollbar-thumb:hover{{background:var(--fg3)}}

/* Animations */
@keyframes fadeIn{{from{{opacity:0;transform:translateY(8px)}}to{{opacity:1;transform:translateY(0)}}}}
</style>
</head>
<body>
<div class=\"app\">
  <aside class=\"side\">
    <div class=\"side-header\">
      <div class=\"brand\">RGA Cowork</div>
      <div class=\"brand-sub\">Rust GenericAgent Runtime</div>
    </div>
    <div class=\"side-body\">
      <form method=\"post\" action=\"/chat\" id=\"chatForm\">
        <label>Provider</label>
        <select name=\"provider\" onchange=\"this.form.submit()\">
          <option value=\"mock\">Mock（无需 Key）</option>
          <option value=\"openai\">OpenAI-compatible / MiniMax</option>
          <option value=\"anthropic\">Anthropic-compatible</option>
        </select>
        <label>API Key</label>
        <input name=\"api_key\" type=\"password\" placeholder=\"sk-...从 mykey.py 或环境变量读取\">
        <label>Base URL</label>
        <input name=\"base_url\" value=\"https://api.minimaxi.com/v1\">
        <label>Model</label>
        <input name=\"model\" value=\"MiniMax-M2.7\">
      </form>
      <div class=\"help-card\"><b>Commands:</b>
{}</div>
    </div>
    <div class=\"side-footer\">
      <form method=\"post\" action=\"/new\">
        <button class=\"btn btn-secondary\" type=\"submit\">清空 / 新对话</button>
      </form>
    </div>
  </aside>
  <main class=\"main\">
    <div class=\"topbar\">
      <div><div class=\"topbar-title\">GenericAgent-style Chat</div><div class=\"topbar-sub\">当前前端: {}</div></div>
      <div class=\"topbar-sub\">RGA v0.1</div>
    </div>
    {}
    {}
    <section class=\"chat\" id=\"chat\">{}</section>
    <section class=\"composer\">
      <textarea name=\"prompt\" form=\"chatForm\" placeholder=\"any task? 输入任务开始执行...\" rows=\"2\"></textarea>
      <button type=\"submit\" form=\"chatForm\" class=\"btn btn-primary\">发送</button>
    </section>
  </main>
</div>
<script>
const c=document.getElementById('chat');
if(c)c.scrollTop=c.scrollHeight;
document.addEventListener('keydown',e=>{{
  if(e.key==='Enter'&&!e.shiftKey){{
    const t=document.querySelector('textarea[name=prompt]');
    if(t&&document.activeElement===t){{e.preventDefault();document.getElementById('chatForm').submit()}}
  }}
}});
</script>
</body>
</html>"#,
        html_escape(&commands_html),
        html_escape(frontend),
        error_html,
        answer_html,
        bubbles
    )
}
