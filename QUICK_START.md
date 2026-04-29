# RGA 快速上手指南

> 从零开始，5 分钟跑起来。

---

## 1. 安装 Rust

如果已经装过 Rust，跳过这步。

**Windows:**

1. 打开 https://rustup.rs/ ，下载 `rustup-init.exe` 并运行
2. 一路默认安装即可
3. 安装完成后**重启终端**，输入以下命令验证：

```bash
rustc --version
cargo --version
```

看到版本号就说明装好了。

---

## 2. 克隆项目

```bash
git clone https://github.com/pangpang778/RGA.git
cd RGA
```

---

## 3. 启动 GUI

```bash
cargo run -- --gui
```

首次运行会下载依赖，编译时间约 1-2 分钟。编译完成后会弹出一个窗口。

**或者直接双击 `start_gui.bat`**（Windows）。

---

## 4. 配置 API

点击窗口右上角的 **齿轮图标** 打开设置面板：

| 字段 | 说明 | 示例 |
|------|------|------|
| **Provider** | 选择 LLM 提供商 | `OpenAI` 或 `Anthropic` |
| **API Key** | 你的 API 密钥 | `sk-...` 或 `sk-ant-...` |
| **Base URL** | API 地址（OpenAI 可选） | `https://api.openai.com/v1` |
| **Model** | 模型名称 | `gpt-4o` 或 `claude-sonnet-4-6` |

填好后点 **保存**，即可开始对话。

### 支持的 Provider

**OpenAI 兼容**（包括各种第三方转发）：

```
Provider:  OpenAI
API Key:   sk-your-key
Base URL:  https://api.openai.com/v1    （或你的转发地址）
Model:     gpt-4o
```

**Anthropic 兼容**：

```
Provider:  Anthropic
API Key:   sk-ant-your-key
Base URL:  留空即可
Model:     claude-sonnet-4-6
```

**国内中转站示例**（以 one-api 为例）：

```
Provider:  OpenAI
API Key:   你的中转站 key
Base URL:  https://your-proxy.com/v1
Model:     gpt-4o 或 claude-sonnet-4-6
```

---

## 5. 开始对话

1. 在底部输入框输入问题
2. 按 **Enter** 或点 **发送**
3. 回复会**实时流式输出**（逐字出现，和 ChatGPT 体验一致）
4. 回复完成后可以继续提问，支持连续对话

---

## 6. 环境变量方式（可选）

也可以通过环境变量配置，GUI 启动时会自动读取：

**PowerShell:**

```powershell
$env:RGA_PROVIDER='openai'
$env:RGA_OPENAI_API_KEY='sk-...'
$env:RGA_OPENAI_BASE_URL='https://api.openai.com/v1'
$env:RGA_OPENAI_MODEL='gpt-4o'
cargo run -- --gui
```

**Bash:**

```bash
export RGA_PROVIDER=openai
export RGA_OPENAI_API_KEY=sk-...
export RGA_OPENAI_BASE_URL=https://api.openai.com/v1
export RGA_OPENAI_MODEL=gpt-4o
cargo run -- --gui
```

---

## 常见问题

### Q: 编译报错怎么办？

确保 Rust 版本是最新的：

```bash
rustup update
```

### Q: 连不上 API？

- 检查 API Key 是否正确
- 检查 Base URL 是否可访问（国内访问 OpenAI 需要代理）
- 如果用中转站，确认 Base URL 格式正确（以 `/v1` 结尾）

### Q: 中文显示乱码？

RGA 会自动加载系统中文字体（微软雅黑/黑体/宋体）。如果显示异常，确认系统有中文字体。

### Q: 怎么用命令行模式？

```bash
# 交互式 REPL
cargo run

# 单次提问
cargo run -- --input "你好"

# 指定 Provider
cargo run -- --provider openai --input "hello"
```

---

## 项目结构

```
RGA/
├── src/
│   ├── main.rs          # 入口 + CLI
│   ├── gui.rs           # egui 原生 GUI
│   ├── llm.rs           # LLM 客户端（OpenAI/Anthropic）
│   ├── agent_loop.rs    # Agent 循环
│   ├── tools.rs         # 内置工具
│   └── ...
├── assets/              # 提示词模板
├── Cargo.toml           # 依赖配置
├── start_gui.bat        # Windows 一键启动
└── QUICK_START.md       # 本文件
```
