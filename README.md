# RGA — Rust GenericAgent Core

RGA is a Rust rewrite of the core runtime from `D:\StudyCode\GenericAgent`.
It covers the GenericAgent runtime, CLI/task/reflect/session surfaces, local tools, LLM providers, launcher/hub/frontend adapters, HTML utilities, memory utilities, and migrated assets.

## Implemented

- `rga --task <name> --input <prompt>` file-IO task mode with `input.txt`, `output.txt`, `outputN.txt`, `reply.txt`, and `[ROUND END]`
- interactive REPL when no `--input`, `--task`, or `--reflect` is present
- `/continue`, `/continue N`, `/new` session-log commands in REPL
- `--bg` background process spawning with `stdout.log` / `stderr.log`
- `--reflect` mode: native scheduled-task JSON scanner for `reflect/scheduler.py`, plus Python reflect-script compatibility probing
- OpenAI-compatible and Anthropic-compatible non-streaming native tool calls
- mock provider for offline smoke tests
- core tools: `code_run`, `file_read`, `file_patch`, `file_write`, `web_scan`, `web_execute_js`, `update_working_checkpoint`, `ask_user`, `start_long_term_update`
- `temp/model_responses/model_responses_<pid>.txt` prompt/response session logging
- copied GenericAgent prompt/tool/memory/browser-bridge/frontend/reflect/plugin assets
- frontend/launcher/hub coverage via `src/frontends.rs`, `src/launcher.rs`, `src/hub.rs`, and `--frontend`
- HTML simplification coverage via `src/html.rs`
- memory utility coverage via `src/memory_utils.rs`
- `temp/<task>/output.txt` with `[ROUND END]` marker in task mode
- tool file writes and command working directories are sandboxed to `temp/` by default

## Provider configuration

RGA infers provider from environment variables, or use `--provider mock|openai|anthropic`.

OpenAI-compatible:

```powershell
$env:RGA_PROVIDER='openai'
$env:RGA_OPENAI_API_KEY='sk-...'
$env:RGA_OPENAI_BASE_URL='https://api.openai.com/v1'
$env:RGA_OPENAI_MODEL='gpt-5.4'
cargo run -- --input "hello"
```

Anthropic-compatible:

```powershell
$env:RGA_PROVIDER='anthropic'
$env:RGA_ANTHROPIC_API_KEY='sk-ant-...'
$env:RGA_ANTHROPIC_MODEL='claude-sonnet-4-6'
cargo run -- --input "hello"
```

Offline smoke test:

```powershell
$env:RGA_REPLY_TIMEOUT_SECS='1'
cargo run -- --provider mock --task smoke --input "hello rust agent"
```

## Safety defaults

- Provider URLs must be HTTPS. Set `RGA_ALLOW_INSECURE_PROVIDER=1` only for trusted local testing.
- Absolute paths and paths escaping the RGA sandbox are rejected unless `RGA_UNSAFE_ALLOW_ABSOLUTE=1` is set.
- `file_write`, `file_patch`, and `code_run.cwd` are restricted to `temp/`.
- `file_read` can read from `temp/`, `memory/`, and `assets/` only.
- Task-mode reply wait defaults to 600 seconds, matching Python behavior; set `RGA_REPLY_TIMEOUT_SECS` for tests.
- Browser bridge access is disabled unless `RGA_ENABLE_BROWSER_BRIDGE=1` is set for a trusted local browser session. The copied upstream extension remains powerful and should not be loaded casually.

## Verification

```powershell
cargo fmt --check
cargo test
cargo build
cargo run -- --help
cargo run -- --frontend streamlit --port 18501
```

## Coverage notes

See `MIGRATION_MATRIX.md` for file-by-file coverage. Intentional safe deltas:

- Vendor GUI/bot SDKs are represented by Rust frontend adapters and the shared runtime surfaces rather than embedding each Python SDK.
- LLM streaming output is not implemented; provider calls are non-streaming.
- Browser scan simplification is a native Rust heuristic plus opt-in browser bridge rather than a byte-for-byte JavaScript DOM optimizer.
- `--llm-no` is accepted but provider selection currently uses `--provider` / environment variables.
- Dangerous process-memory/OCR/model-dependent helpers expose safe explicit Result surfaces unless an external backend is configured.
