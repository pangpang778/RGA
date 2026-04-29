# RGA 用户使用指南（第一次上手版）

RGA 是 GenericAgent 的 Rust 重写版。第一次使用推荐先打开本地 GUI：不用记命令，也可以直接配置 MiniMax。

项目目录：

```powershell
D:\StudyCode\RGA
```

---

## 1. 进入项目目录

```powershell
cd D:\StudyCode\RGA
```

---

## 2. 检查 Rust 环境

```powershell
cargo --version
rustc --version
```

能看到版本号即可。

---

## 3. 启动本地 GUI（推荐第一次使用）

```powershell
cargo run -- --frontend streamlit --port 18501
```

然后打开浏览器：

```text
http://127.0.0.1:18501
```

页面里可以直接填写：

- Provider：`Mock`、`OpenAI-compatible / MiniMax`、`Anthropic-compatible`
- API Key：你的模型 Key；也可以留空，使用环境变量
- Base URL：MiniMax 用 `https://api.minimaxi.com/v1`
- Model：MiniMax 用 `MiniMax-M2.7`
- Prompt：输入你的问题

### 无 Key 测试 GUI

Provider 选 `Mock`，Prompt 输入：

```text
hello gui
```

点击发送。如果页面显示回复，说明 GUI 可用。

---

## 4. 在 GUI 里使用 MiniMax

启动 GUI 后，在页面填写：

```text
Provider: OpenAI-compatible / MiniMax
API Key: 你的 MiniMax Key
Base URL: https://api.minimaxi.com/v1
Model: MiniMax-M2.7
Prompt: 请只回答：RGA_OK
```

返回 `RGA_OK` 就说明 MiniMax 已接通。

> 不建议把 API Key 写入仓库文件。GUI 表单只在当前请求中使用。

---

## 5. 命令行：离线测试

如果不用 GUI，也可以命令行测试：

```powershell
cargo run -- --provider mock --input "hello"
```

---

## 6. 命令行：配置 MiniMax

```powershell
$env:RGA_PROVIDER='openai'
$env:RGA_OPENAI_API_KEY='你的 MiniMax API Key'
$env:RGA_OPENAI_BASE_URL='https://api.minimaxi.com/v1'
$env:RGA_OPENAI_MODEL='MiniMax-M2.7'

cargo run -- --provider openai --input "请只回答：RGA_OK" --max-turns 2
```

---

## 7. 让模型使用工具写文件

```powershell
cargo run -- --provider openai --input "请使用 file_write 工具创建文件 first_run/hello.txt，content 参数必须是 Hello RGA，然后结束。" --max-turns 4
```

生成文件：

```text
D:\StudyCode\RGA\temp\first_run\hello.txt
```

查看：

```powershell
Get-Content D:\StudyCode\RGA\temp\first_run\hello.txt
```

---

## 8. Task 文件模式

适合自动化任务：

```powershell
$env:RGA_REPLY_TIMEOUT_SECS='1'
cargo run -- --provider mock --task smoke --input "hello task" --max-turns 2
```

输出：

```text
D:\StudyCode\RGA\temp\smoke\output.txt
```

多轮继续：往下面文件写入内容即可：

```text
D:\StudyCode\RGA\temp\smoke\reply.txt
```

---

## 9. 交互模式

```powershell
cargo run -- --provider openai
```

退出：

```text
/exit
```

查看历史会话：

```text
/continue
```

开启新会话：

```text
/new
```

---

## 10. 后台运行

```powershell
cargo run -- --provider openai --task bg_demo --input "后台任务测试" --bg
```

日志：

```text
D:\StudyCode\RGA\temp\bg_demo\stdout.log
D:\StudyCode\RGA\temp\bg_demo\stderr.log
```

---

## 11. Reflect / 定时任务

任务 JSON 放在：

```text
D:\StudyCode\RGA\sche_tasks\xxx.json
```

示例：

```json
{
  "enabled": true,
  "repeat": "daily",
  "schedule": "09:00",
  "prompt": "每天检查项目状态并写报告",
  "max_delay_hours": 6
}
```

运行：

```powershell
cargo run -- --provider openai --reflect reflect/scheduler.py
```

---

## 12. 安全默认值

RGA 默认比较保守：

- Provider URL 必须是 `https://`
- `file_write` / `file_patch` / `code_run.cwd` 限制在 `temp/`
- `file_read` 只允许读 `temp/`、`memory/`、`assets/`
- 浏览器 bridge 默认关闭
- 绝对路径默认拒绝

---

## 13. 浏览器 bridge

只有信任本地浏览器环境时才开启：

```powershell
$env:RGA_ENABLE_BROWSER_BRIDGE='1'
```

---

## 14. 常见问题

### Q：第一次使用到底该运行什么？

运行 GUI：

```powershell
cd D:\StudyCode\RGA
cargo run -- --frontend streamlit --port 18501
```

打开：

```text
http://127.0.0.1:18501
```

### Q：为什么文件没写到 D:\xxx？

默认写入 `D:\StudyCode\RGA\temp\`，这是安全沙箱。

### Q：task 模式为什么等很久？

它在等 `reply.txt`，默认 600 秒。测试时设置：

```powershell
$env:RGA_REPLY_TIMEOUT_SECS='1'
```

### Q：如何确认项目没坏？

```powershell
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test
cargo build
```

---

## 15. 推荐第一次流程

```powershell
cd D:\StudyCode\RGA
cargo run -- --frontend streamlit --port 18501
```

浏览器打开：

```text
http://127.0.0.1:18501
```

先用 Mock 测试，再填 MiniMax：

```text
Base URL: https://api.minimaxi.com/v1
Model: MiniMax-M2.7
```
