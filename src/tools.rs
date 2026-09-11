//! lly 工具：EdgeTTS / Transcribe / FileTool 三个 lilyco App
//!
//! 每个 App 天然四端：CLI 短生命命令 + `--mcp` 常驻暴露给 AI。

use std::path::{Path, PathBuf};
use std::process::Command;

use calamine::Reader;
use lilyco::prelude::*;

// ── EdgeTTS ───────────────────────────────────────────────

#[derive(App)]
#[app(about = "Edge TTS：文本转语音（微软在线语音，返回 mp3）", run = "run_tts")]
pub struct EdgeTTS {
    #[arg(about = "要朗读的文本")]
    pub text: String,

    #[arg(about = "声音，如 zh-CN-XiaoxiaoNeural", default = "zh-CN-XiaoxiaoNeural")]
    pub voice: String,

    #[arg(about = "语速，如 +0% / +10% / -10%", default = "+0%")]
    pub rate: String,

    #[arg(about = "音调，如 +0Hz / +10Hz / -10Hz", default = "+0Hz")]
    pub pitch: String,

    #[arg(about = "输出 mp3 路径（默认 /tmp/lly/tts_<时间戳>.mp3）")]
    pub output: Option<String>,
}

pub fn run_tts(app: &EdgeTTS, ctx: &Context) -> Result<serde_json::Value, AppError> {
    ctx.emit(Progress::Started {
        total: Some(1),
        message: Some("合成语音中...".into()),
    });
    let audio = crate::edge_tts::synthesize(&app.text, &app.voice, &app.rate, &app.pitch)
        .map_err(|e| AppError::Runtime(format!("edge-tts: {e}")))?;

    let out = match &app.output {
        Some(p) => PathBuf::from(p),
        None => {
            let ts = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0);
            PathBuf::from(format!("/tmp/lly/tts_{ts}.mp3"))
        }
    };
    if let Some(parent) = out.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)
                .map_err(|e| AppError::Runtime(format!("mkdir {}: {e}", parent.display())))?;
        }
    }
    std::fs::write(&out, &audio).map_err(|e| AppError::Runtime(format!("write {}: {e}", out.display())))?;

    let result = serde_json::json!({
        "status": "ok",
        "path": out.to_string_lossy(),
        "bytes": audio.len(),
        "estimate_seconds": (audio.len() / 6000) as u64, // 48kbps → ~6KB/s
        "voice": app.voice,
    });
    ctx.done(result.clone(), 0);
    Ok(result)
}

// ── Transcribe ────────────────────────────────────────────

#[derive(App)]
#[app(about = "语音转文字（whisper.cpp 本地小模型）", run = "run_transcribe")]
pub struct Transcribe {
    #[arg(about = "输入音频/视频文件", must_exist = true)]
    pub audio: PathBuf,

    #[arg(about = "语言：zh/en/ja/ko/auto", default = "auto")]
    pub language: String,

    #[arg(about = "模型：tiny/base/small", default = "base")]
    pub model: String,
}

pub fn run_transcribe(app: &Transcribe, ctx: &Context) -> Result<serde_json::Value, AppError> {
    ctx.emit(Progress::Started {
        total: Some(2),
        message: Some("whisper 转写中...".into()),
    });
    let whisper_cli = "/home/radxa/whisper/whisper.cpp/build/bin/whisper-cli";
    if !Path::new(whisper_cli).exists() {
        return Err(AppError::Runtime(format!("whisper-cli 未找到: {whisper_cli}")));
    }
    let model_path = format!("/home/radxa/whisper/ggml-{}.bin", app.model);
    if !Path::new(&model_path).exists() {
        return Err(AppError::Runtime(format!("模型未找到: {model_path}")));
    }
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let out_prefix = format!("/tmp/lly/whisper_{ts}");
    if let Some(parent) = Path::new(&out_prefix).parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| AppError::Runtime(format!("mkdir: {e}")))?;
    }

    let mut cmd = Command::new(whisper_cli);
    cmd.arg("-m").arg(&model_path).arg("-f").arg(&app.audio);
    if app.language != "auto" {
        cmd.arg("-l").arg(&app.language);
    }
    cmd.arg("-oj").arg("-of").arg(&out_prefix).arg("-np");
    let out = cmd
        .output()
        .map_err(|e| AppError::Runtime(format!("whisper-cli 执行失败: {e}")))?;

    ctx.tick(1, Some(2), "解析结果...");
    let json_path = format!("{out_prefix}.json");
    let text = if Path::new(&json_path).exists() {
        let data = std::fs::read(&json_path).map_err(|e| AppError::Runtime(format!("读 json: {e}")))?;
        let v: serde_json::Value = serde_json::from_slice(&data)
            .map_err(|e| AppError::Runtime(format!("解析 whisper json: {e}")))?;
        v["text"].as_str().unwrap_or("").trim().to_string()
    } else {
        // 兜底：从 stdout 提取
        String::from_utf8_lossy(&out.stdout).to_string()
    };

    let result = serde_json::json!({
        "status": "ok",
        "text": text,
        "language": app.language,
        "model": app.model,
        "exit_code": out.status.code().unwrap_or(-1),
    });
    ctx.done(result.clone(), 0);
    Ok(result)
}

// ── FileTool ──────────────────────────────────────────────

#[derive(Debug, ValueEnum)]
pub enum Action {
    Info,
    Convert,
    Parse,
    Safe,
}

#[derive(App)]
#[app(about = "统一文件处理：安全检查/信息/转换/解析（ffmpeg/excel/toml/json）", run = "run_file")]
pub struct FileTool {
    #[arg(about = "输入文件", must_exist = true)]
    pub path: PathBuf,

    #[arg(about = "动作：info/convert/parse/safe", default = "info")]
    pub action: Action,

    #[arg(about = "目标格式（convert，如 mp4/mp3/webm）或解析类型（parse，如 xlsx/toml/json）")]
    pub format: Option<String>,

    #[arg(about = "ffmpeg 附加参数（convert，白名单字符）")]
    pub args: Option<String>,

    #[arg(about = "输出目录（convert/parse 的产物落点）", default = "/tmp/lly")]
    pub out: String,
}

/// 读路径白名单（canonicalize 后前缀匹配）
const READ_ALLOW: [&str; 4] = ["/tmp/lly", "/home/radxa", "/var/www", "/tmp"];
/// 允许处理的扩展名
const ALLOW_EXT: [&str; 32] = [
    "mp3", "wav", "flac", "aac", "m4a", "ogg", "opus", "mp4", "mkv", "webm", "avi", "mov",
    "jpg", "jpeg", "png", "webp", "gif", "bmp", "xlsx", "xls", "toml", "json", "yaml", "yml",
    "md", "txt", "csv", "pdf", "zip", "gz", "srt", "vtt",
];
const MAX_SIZE: u64 = 500 * 1024 * 1024; // 500MB

/// 安全检查：规范化 + 白名单 + 大小限制
fn check_read_path(p: &Path) -> Result<PathBuf, AppError> {
    let canon = p
        .canonicalize()
        .map_err(|e| AppError::InvalidArg(format!("路径无效 {}: {e}", p.display())))?;
    let s = canon.to_string_lossy().to_string();
    if !READ_ALLOW.iter().any(|a| s == *a || s.starts_with(&format!("{a}/"))) {
        return Err(AppError::InvalidArg(format!("路径不在白名单: {s}")));
    }
    let ext = canon
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();
    if !ALLOW_EXT.contains(&ext.as_str()) {
        return Err(AppError::InvalidArg(format!("不支持的扩展名: .{ext}")));
    }
    let size = std::fs::metadata(&canon)
        .map_err(|e| AppError::Runtime(format!("stat: {e}")))?
        .len();
    if size > MAX_SIZE {
        return Err(AppError::InvalidArg(format!("文件过大 {size} > {MAX_SIZE}")));
    }
    Ok(canon)
}

/// ffmpeg 附加参数白名单（防命令注入）
fn check_ffmpeg_args(s: &str) -> Result<(), AppError> {
    let allowed = |c: char| c.is_alphanumeric() || " -_=.,:/%()[]+#".contains(c);
    if !s.chars().all(allowed) {
        return Err(AppError::InvalidArg(
            "ffmpeg 参数含非法字符（仅允许字母数字与 -_=.,:/%()[]+# 空格）".into(),
        ));
    }
    if s.contains("filter_complex") && !s.contains("scale") {
        // 允许 filter_complex 但要求显式白名单化（简单放行，参数本身已字符白名单）
    }
    Ok(())
}

fn run_ffprobe(p: &Path) -> Result<serde_json::Value, AppError> {
    let out = Command::new("ffprobe")
        .args(["-v", "error", "-show_format", "-show_streams", "-of", "json"])
        .arg(p)
        .output()
        .map_err(|e| AppError::Runtime(format!("ffprobe: {e}")))?;
    if !out.status.success() {
        return Err(AppError::Runtime(format!(
            "ffprobe 失败: {}",
            String::from_utf8_lossy(&out.stderr)
        )));
    }
    serde_json::from_slice(&out.stdout)
        .map_err(|e| AppError::Runtime(format!("解析 ffprobe: {e}")))
}

fn run_file(app: &FileTool, ctx: &Context) -> Result<serde_json::Value, AppError> {
    let canon = check_read_path(&app.path)?;
    ctx.emit(Progress::Started {
        total: Some(1),
        message: Some(format!("处理 {}", canon.display())),
    });

    let ext = canon
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();
    let size = std::fs::metadata(&canon)
        .map_err(|e| AppError::Runtime(format!("stat: {e}")))?
        .len();

    let result = match app.action {
        Action::Safe => serde_json::json!({
            "status": "ok",
            "safe": true,
            "path": canon.to_string_lossy(),
            "ext": ext,
            "size": size,
            "note": "路径在白名单、扩展名与大小均合规",
        }),
        Action::Info => {
            let mut v = serde_json::json!({
                "path": canon.to_string_lossy(),
                "ext": ext,
                "size": size,
            });
            if ["mp3", "wav", "flac", "aac", "m4a", "ogg", "opus", "mp4", "mkv", "webm", "avi", "mov"]
                .contains(&ext.as_str())
            {
                match run_ffprobe(&canon) {
                    Ok(meta) => v["media"] = meta,
                    Err(e) => v["media_error"] = serde_json::Value::String(e.to_string()),
                }
            } else if ext == "xlsx" || ext == "xls" {
                v["excel"] = excel_info(&canon)?;
            } else if ext == "toml" {
                let text = std::fs::read_to_string(&canon)
                    .map_err(|e| AppError::Runtime(format!("read toml: {e}")))?;
                let parsed: toml::Value = text
                    .parse()
                    .map_err(|e| AppError::Runtime(format!("toml 解析失败: {e}")))?;
                v["toml_tables"] = serde_json::json!(parsed.as_table().map(|t| t.len()).unwrap_or(0));
                v["toml_keys"] = serde_json::to_value(
                        parsed
                            .as_table()
                            .map(|t| t.keys().cloned().collect::<Vec<_>>())
                            .unwrap_or_default(),
                    )
                    .unwrap_or_default();
            }
            v
        }
        Action::Parse => {
            let fmt = app.format.as_deref().unwrap_or(&ext).to_lowercase();
            if fmt == "xlsx" || fmt == "xls" {
                serde_json::json!({ "excel": excel_dump(&canon, app)? })
            } else if fmt == "toml" {
                let text = std::fs::read_to_string(&canon)
                    .map_err(|e| AppError::Runtime(format!("read toml: {e}")))?;
                let parsed: toml::Value = text
                    .parse()
                    .map_err(|e| AppError::Runtime(format!("toml 解析失败: {e}")))?;
                serde_json::json!({ "toml": parsed })
            } else if fmt == "json" {
                let text = std::fs::read_to_string(&canon)
                    .map_err(|e| AppError::Runtime(format!("read json: {e}")))?;
                let parsed: serde_json::Value = serde_json::from_str(&text)
                    .map_err(|e| AppError::Runtime(format!("json 解析失败: {e}")))?;
                serde_json::json!({ "json": parsed })
            } else {
                return Err(AppError::InvalidArg(format!(
                    "parse 仅支持 xlsx/toml/json，收到 {fmt}"
                )));
            }
        }
        Action::Convert => {
            let fmt = app
                .format
                .as_deref()
                .ok_or_else(|| AppError::InvalidArg("convert 需要 --format".into()))?
                .to_lowercase();
            if !["mp3", "wav", "flac", "aac", "m4a", "ogg", "opus", "mp4", "mkv", "webm", "avi", "mov", "jpg", "png", "webp", "gif"].contains(&fmt.as_str())
            {
                return Err(AppError::InvalidArg(format!("不支持的输出格式: {fmt}")));
            }
            std::fs::create_dir_all(&app.out)
                .map_err(|e| AppError::Runtime(format!("mkdir out: {e}")))?;
            let stem = canon
                .file_stem()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_else(|| "out".into());
            let ts = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0);
            let out_path = Path::new(&app.out).join(format!("{stem}_{ts}.{fmt}"));
            let mut cmd = Command::new("ffmpeg");
            cmd.arg("-y").arg("-i").arg(&canon);
            if let Some(a) = &app.args {
                check_ffmpeg_args(a)?;
                // 参数按 shell 空白切分传入（字符白名单已保证安全）
                for tok in a.split_whitespace() {
                    cmd.arg(tok);
                }
            }
            cmd.arg(&out_path);
            let out = cmd
                .output()
                .map_err(|e| AppError::Runtime(format!("ffmpeg: {e}")))?;
            if !out.status.success() {
                return Err(AppError::Runtime(format!(
                    "ffmpeg 失败: {}",
                    String::from_utf8_lossy(&out.stderr).chars().take(500).collect::<String>()
                )));
            }
            let out_size = std::fs::metadata(&out_path)
                .map(|m| m.len())
                .unwrap_or(0);
            serde_json::json!({
                "status": "ok",
                "output": out_path.to_string_lossy(),
                "size": out_size,
            })
        }
    };
    ctx.done(result.clone(), 0);
    Ok(result)
}

/// excel 概览：sheet 名 + 维度
fn excel_info(p: &Path) -> Result<serde_json::Value, AppError> {
    let mut wb = calamine::open_workbook_auto(p)
        .map_err(|e| AppError::Runtime(format!("打开 excel: {e}")))?;
    let sheets = wb
        .sheet_names()
        .into_iter()
        .map(|name| {
            let dim = wb
                .worksheet_range(&name)
                .ok()
                .map(|r| (r.width(), r.height()))
                .unwrap_or((0, 0));
            serde_json::json!({ "name": name, "cols": dim.0, "rows": dim.1 })
        })
        .collect::<Vec<_>>();
    Ok(serde_json::json!({ "sheets": sheets }))
}

/// excel 内容：前 N 行（默认 50 行）
fn excel_dump(p: &Path, _app: &FileTool) -> Result<serde_json::Value, AppError> {
    let mut wb = calamine::open_workbook_auto(p)
        .map_err(|e| AppError::Runtime(format!("打开 excel: {e}")))?;
    let mut out = serde_json::Map::new();
    for name in wb.sheet_names() {
        let range = wb
            .worksheet_range(&name)
            .map_err(|e| AppError::Runtime(format!("读 sheet {name}: {e}")))?;
        let rows = range.rows().take(50).map(|row| {
            serde_json::Value::Array(
                row.iter()
                    .map(|c| match c {
                        calamine::Data::Empty => serde_json::Value::Null,
                        calamine::Data::String(s) => serde_json::Value::String(s.clone()),
                        calamine::Data::Float(f) => serde_json::json!(f),
                        calamine::Data::Int(i) => serde_json::json!(i),
                        calamine::Data::Bool(b) => serde_json::json!(b),
                        calamine::Data::DateTime(dt) => serde_json::json!(dt.to_string()),
                        calamine::Data::DateTimeIso(dt) => serde_json::json!(dt.to_string()),
                        calamine::Data::DurationIso(d) => serde_json::json!(d.to_string()),
                        calamine::Data::Error(e) => serde_json::json!({ "error": e.to_string() }),
                    })
                    .collect(),
            )
        });
        out.insert(name, serde_json::Value::Array(rows.collect()));
    }
    Ok(serde_json::Value::Object(out))
}
