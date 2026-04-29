# GenericAgent -> RGA rewrite coverage

## Native Rust rewrites

- `agentmain.py` -> `src/main.rs`, `src/agent_loop.rs`
  - CLI, REPL, task file loop, background spawning, reflect loop, provider selection.
- `agent_loop.py` -> `src/agent_loop.rs`
  - Turn loop, native tool-call dispatch, tool results, final/max-turn exits.
- `llmcore.py` -> `src/llm.rs`, `src/config.rs`, `src/session_log.rs`
  - OpenAI-compatible chat completions, Anthropic messages, tool-call parsing, provider URL handling, model response logs, `/continue` listing.
- `ga.py` core tools -> `src/tools.rs`
  - Code execution, file read/write/patch, working checkpoint, ask_user, long-term update, browser bridge calls.
- `reflect/scheduler.py` -> `src/scheduler.rs`
  - `sche_tasks/*.json` scan, repeat/cooldown/window handling, report prompt generation.
- `frontends/chatapp_common.py` and shared frontend helpers -> `src/frontends.rs`
  - Help commands, reply cleaning, file markers, text splitting, frontend registry, skins discovery.
- `hub.pyw` -> `src/hub.rs`
  - Service discovery, singleton lock probing, service manager, frontend command mapping.
- `launch.pyw` -> `src/launcher.rs`
  - Free-port discovery, prompt injection files, web-runtime launch helpers, last-reply time.
- `simphtml.py` -> `src/html.rs`
  - HTML noise stripping, text extraction, link extraction, changed-text detection, truncation.
- `memory/keychain.py`, `skill_search`, OCR/UI/procmem utility surfaces -> `src/memory_utils.rs`
  - Obfuscated keychain, environment detection, local skill search; high-risk OCR/model/procmem operations expose safe explicit Result surfaces.
- Prompt/schema/memory/frontend assets -> `assets/`, `memory/`, `frontends_assets/`, `reflect_assets/`, `plugins_assets/`
  - Tool schemas, system prompts, memory templates, SOP files, skins/images, mykey templates, docs.

## Compatibility surfaces

- `--bg`: native process spawning, PID print, `stdout.log` / `stderr.log`.
- `--task`: `input.txt`, `output.txt`, `outputN.txt`, `reply.txt`, `[ROUND END]`.
- `--reflect`: native scheduler for bundled scheduler; subprocess compatibility for arbitrary Python reflect scripts.
- `--frontend`: native local web adapter and registry for streamlit/Qt/pet/bot frontends.
- `/continue`: session log listing and current-log snapshotting.
- Browser bridge: opt-in HTTP bridge client plus copied upstream extension assets.

## Safety/intentional deltas

- GUI/bot SDKs are represented by Rust frontend adapters and shared runtime surfaces, not by embedding each vendor's Python SDK.
- HTML simplification is a native Rust heuristic port, not a byte-for-byte JavaScript DOM optimizer.
- Real-time LLM streaming display is not ported; provider calls are non-streaming.
- Dangerous process-memory/OCR/model-dependent helpers expose safe Result surfaces unless an external backend is configured.
