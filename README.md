<p align="right">
  <a href="#english">English</a> | <a href="#中文">中文</a>
</p>

---

# RGA — Rust GenericAgent

<p align="center">
  <strong>原生桌面 AI 助手 | 流式对话 | 多模型支持</strong>
</p>

---

<a id="中文"></a>

## 中文

### 简介

RGA 是 GenericAgent 的 Rust 重写版本，提供原生桌面 GUI，支持 OpenAI / Anthropic 及其中转站，实时流式输出，开箱即用。

### 截图

> 启动后点击右上角齿轮图标配置 API，即可开始对话。

### 快速开始

#### 1. 安装 Rust

已安装则跳过。

```bash
# Windows: 下载 https://rustup.rs/ 并运行 rustup-init.exe
# 或使用 winget
winget install Rustlang.Rustup

# macOS / Linux
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

验证：

```bash
rustc --version
cargo --version
```

#### 2. 克隆并运行

```bash
git clone https://github.com/pangpang778/RGA.git
cd RGA
cargo run -- --gui
```

首次编译约 1-2 分钟，之后秒启动。

Windows 用户也可以双击 `start_gui.bat`。

#### 3. 配置 API

点击窗口右上角 **齿轮图标**，填入：

| 字段 | 说明 | 示例 |
|------|------|------|
| Provider | LLM 提供商 | `OpenAI` / `Anthropic` |
| API Key | 你的密钥 | `sk-...` / `sk-ant-...` |
| Base URL | API 地址 | `https://api.openai.com/v1` |
| Model | 模型名称 | `gpt-4o` / `claude-sonnet-4-6` |

支持 OpenAI 兼容接口和 Anthropic 原生接口，国内中转站只需修改 Base URL。

#### 4. 开始对话

- 输入框输入问题，按 **Enter** 发送
- 回复实时流式输出（逐字出现）
- 支持连续多轮对话
- 右上角齿轮可随时修改配置

### 命令行模式

```bash
# 交互式 REPL
cargo run

# 单次提问
cargo run -- --input "你好"

# 指定 Provider
cargo run -- --provider openai --input "hello"
```

### 环境变量配置

```powershell
# PowerShell
$env:RGA_PROVIDER='openai'
$env:RGA_OPENAI_API_KEY='sk-...'
$env:RGA_OPENAI_BASE_URL='https://api.openai.com/v1'
$env:RGA_OPENAI_MODEL='gpt-4o'
```

```bash
# Bash
export RGA_PROVIDER=openai
export RGA_OPENAI_API_KEY=sk-...
export RGA_OPENAI_BASE_URL=https://api.openai.com/v1
export RGA_OPENAI_MODEL=gpt-4o
```

### 项目结构

```
src/
├── main.rs          # 入口 + CLI
├── gui.rs           # egui 原生 GUI（流式对话）
├── llm.rs           # LLM 客户端（OpenAI / Anthropic，SSE 流式）
├── agent_loop.rs    # Agent 多轮循环
├── tools.rs         # 内置工具（代码执行、文件读写、网页扫描）
├── config.rs        # 配置管理
├── memory_utils.rs  # 记忆工具
└── ...
```

### 常见问题

**编译报错？** → `rustup update` 更新 Rust 到最新版。

**连不上 API？** → 检查 API Key、Base URL，国内访问 OpenAI 需要代理或中转站。

**中文乱码？** → RGA 自动加载系统中文字体（微软雅黑/黑体/宋体）。

### License

MIT

---

<a id="english"></a>

## English

### Introduction

RGA is a Rust rewrite of GenericAgent — a native desktop AI assistant with real-time SSE streaming, supporting OpenAI / Anthropic APIs and their proxies.

### Quick Start

#### 1. Install Rust

Skip if already installed.

```bash
# Windows: download from https://rustup.rs/ and run rustup-init.exe
# or use winget
winget install Rustlang.Rustup

# macOS / Linux
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

Verify:

```bash
rustc --version
cargo --version
```

#### 2. Clone and Run

```bash
git clone https://github.com/pangpang778/RGA.git
cd RGA
cargo run -- --gui
```

First build takes ~1-2 minutes, subsequent launches are instant.

Windows users can also double-click `start_gui.bat`.

#### 3. Configure API

Click the **gear icon** (top-right) to open settings:

| Field | Description | Example |
|-------|-------------|---------|
| Provider | LLM provider | `OpenAI` / `Anthropic` |
| API Key | Your API key | `sk-...` / `sk-ant-...` |
| Base URL | API endpoint | `https://api.openai.com/v1` |
| Model | Model name | `gpt-4o` / `claude-sonnet-4-6` |

Supports OpenAI-compatible and Anthropic native APIs. Third-party proxies work by changing Base URL.

#### 4. Start Chatting

- Type your question and press **Enter** to send
- Responses stream in real-time (token by token)
- Continuous multi-turn conversation supported
- Update settings anytime via the gear icon

### CLI Mode

```bash
# Interactive REPL
cargo run

# Single question
cargo run -- --input "hello"

# Specify provider
cargo run -- --provider openai --input "hello"
```

### Environment Variables

```powershell
# PowerShell
$env:RGA_PROVIDER='openai'
$env:RGA_OPENAI_API_KEY='sk-...'
$env:RGA_OPENAI_BASE_URL='https://api.openai.com/v1'
$env:RGA_OPENAI_MODEL='gpt-4o'
```

```bash
# Bash
export RGA_PROVIDER=openai
export RGA_OPENAI_API_KEY=sk-...
export RGA_OPENAI_BASE_URL=https://api.openai.com/v1
export RGA_OPENAI_MODEL=gpt-4o
```

### Project Structure

```
src/
├── main.rs          # Entry point + CLI
├── gui.rs           # Native egui GUI (streaming chat)
├── llm.rs           # LLM clients (OpenAI / Anthropic, SSE streaming)
├── agent_loop.rs    # Multi-turn agent loop
├── tools.rs         # Built-in tools (code exec, file R/W, web scan)
├── config.rs        # Configuration
├── memory_utils.rs  # Memory utilities
└── ...
```

### FAQ

**Build errors?** → Run `rustup update` to get the latest Rust.

**Can't connect to API?** → Verify API key and Base URL. Use a proxy if needed.

**CJK text issues?** → RGA auto-loads system CJK fonts (MS YaHei, SimHei, SimSun).

### License

MIT
