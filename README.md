# lly —— Radxa 工具包（lilyco 框架封装）

EdgeTTS 语音合成 / Whisper 本地转写 / 统一文件处理 —— 一个 struct 派生 CLI/TUI/Web/MCP 四端，天然可被 Agent 调用。

## 工具

| 命令 | 说明 | 内核 |
|---|---|---|
| `lly tts --text … [--voice … --rate …]` | 微软 Edge 在线语音合成，输出 mp3 | [msedge-tts](https://github.com/hs-CN/msedge-tts) crate（DRM 令牌/握手已封装） |
| `lly transcribe --audio <file> [--language zh --model base]` | 本地语音转文字（whisper.cpp，无网络） | whisper.cpp ggml 模型 |
| `lly file --path <file> --action info\|safe\|convert\|parse [--format …]` | 统一文件处理：信息/安全检查/转换/解析（ffmpeg/excel/toml/json） | ffmpeg + calamine + toml |

## 运行模式

```bash
lly --mcp            # MCP 服务器（Agent 直接调用三工具）
lly --serve 9901     # HTTP 网关（算力平台任务入口）
lly --list           # 打印注册表 schema JSON
lly tts --text "你好" --voice zh-CN-XiaoxiaoNeural --output out.mp3
```

- 所有工具经 lilyco `Registry` 注册：schema 自描述、参数校验共享（MCP `INVALID_PARAMS` / Web 400 / TUI 拦截）
- EdgeTTS 内核 v0.2.0 起改用 msedge-tts crate（自研协议层退役，修复 403/rustls/时钟问题）

## 安装

```bash
# 预编译 (CI Release, aarch64-musl / windows)
curl -fLO https://github.com/lilyco-42/lly/releases/latest/download/lly-aarch64-unknown-linux-musl.tar.gz
tar xzf lly-aarch64-unknown-linux-musl.tar.gz && install -m755 lly ~/.local/bin/

# 或源码构建 (Rust 工具链)
git clone https://github.com/lilyco-42/lly && cd lly && cargo build --release
```

whisper 转写需本地 ggml 模型（如 ggml-base.bin）与 whisper.cpp 的 `main`/`whisper-cli` 二进制。

## 许可

MIT OR Apache-2.0
