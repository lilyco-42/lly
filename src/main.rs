//! lly — Radxa 算力平台工具包（lilyco 框架）
//!
//! 用法：
//! ```bash
//! lly --mcp                                   # MCP 常驻服务器（AI 直接调用 3 个工具）
//! lly --list                                  # 打印注册表 schema JSON
//! lly --serve 9901                            # HTTP 网关（compute 平台任务入口）
//! lly tts --text "你好" --voice zh-CN-XiaoxiaoNeural
//! lly transcribe --audio a.mp3 --language zh
//! lly file --path x.xlsx --action info
//! lly file --path v.mp4 --action convert --format mp3
//! ```

mod edge_tts;
mod tools;

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};

use lilyco::prelude::*;

fn build_registry() -> Registry {
    let mut r = Registry::new();
    for cmd in [
        RegisteredCommand::from_app::<tools::EdgeTTS>().alias("tts"),
        RegisteredCommand::from_app::<tools::Transcribe>().alias("transcribe"),
        RegisteredCommand::from_app::<tools::FileTool>().alias("file"),
    ] {
        r.register(cmd).expect("register lly tool");
    }
    r
}

// ── 极简 HTTP 网关（compute 平台任务入口）──────────────────

fn serve(port: u16) {
    let listener = TcpListener::bind(("0.0.0.0", port))
        .unwrap_or_else(|e| {
            eprintln!("bind :{port} 失败: {e}");
            std::process::exit(1);
        });
    eprintln!("lly gateway listening on :{port}");
    for stream in listener.incoming() {
        match stream {
            Ok(s) => {
                std::thread::spawn(move || handle_conn(s));
            }
            Err(e) => eprintln!("accept error: {e}"),
        }
    }
}

fn handle_conn(mut stream: TcpStream) {
    let registry = build_registry();
    let mut reader = BufReader::new(stream.try_clone().unwrap_or_else(|_| unreachable!()));
    let mut req_line = String::new();
    if reader.read_line(&mut req_line).is_err() {
        return;
    }
    let mut parts = req_line.split_whitespace();
    let _method = parts.next().unwrap_or("");
    let path = parts.next().unwrap_or("/").to_string();

    let mut content_length: usize = 0;
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line).is_err() || line == "\r\n" || line == "\n" {
            break;
        }
        let low = line.to_lowercase();
        if let Some(v) = low.strip_prefix("content-length:") {
            content_length = v.trim().parse().unwrap_or(0);
        }
    }
    let mut body = vec![0u8; content_length];
    if content_length > 0 && reader.read_exact(&mut body).is_err() {
        return;
    }

    let name = path.trim_start_matches('/');
    if let Some(cmd) = registry.get(name) {
        if let Some(handler) = &cmd.handler {
            let args_val: serde_json::Value = if body.is_empty() {
                serde_json::json!({})
            } else {
                match serde_json::from_slice(&body) {
                    Ok(v) => v,
                    Err(e) => {
                        write_json(
                            &mut stream,
                            400,
                            &serde_json::json!({"status":"error","error":format!("body 非 JSON: {e}")}),
                        );
                        return;
                    }
                }
            };
            let (tx, _rx) = std::sync::mpsc::channel();
            let ctx = Context::new_test(tx);
            match handler(&ctx, &args_val) {
                Ok(v) => {
                    write_json(&mut stream, 200, &v);
                    return;
                }
                Err(e) => {
                    write_json(&mut stream, 500, &serde_json::json!({"status":"error","error":e.to_string()}));
                    return;
                }
            }
        }
    }
    write_json(
        &mut stream,
        404,
        &serde_json::json!({"status":"error","error":format!("未知工具: {name}")}),
    );
}

fn write_json(stream: &mut TcpStream, status: u16, v: &serde_json::Value) {
    let body = serde_json::to_vec(v).unwrap_or_default();
    let reason = if status == 200 {
        "OK"
    } else if status == 400 {
        "Bad Request"
    } else if status == 404 {
        "Not Found"
    } else {
        "Internal Server Error"
    };
    let _ = write!(
        stream,
        "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    let _ = stream.write_all(&body);
    let _ = stream.flush();
}

// ── 手动 CLI dispatch ────────────────────────────────────

fn parse_cli(args: &[String]) -> (String, std::collections::HashMap<String, serde_json::Value>) {
    let name = args.first().cloned().unwrap_or_default();
    let mut map = std::collections::HashMap::new();
    let mut i = 1;
    while i < args.len() {
        let a = &args[i];
        if let Some(key) = a.strip_prefix("--") {
            if i + 1 < args.len() {
                let v = &args[i + 1];
                map.insert(
                    key.to_string(),
                    if v == "true" {
                        serde_json::Value::Bool(true)
                    } else if v == "false" {
                        serde_json::Value::Bool(false)
                    } else if let Ok(n) = v.parse::<i64>() {
                        serde_json::json!(n)
                    } else if let Ok(f) = v.parse::<f64>() {
                        serde_json::json!(f)
                    } else {
                        serde_json::Value::String(v.clone())
                    },
                );
                i += 2;
            } else {
                map.insert(key.to_string(), serde_json::Value::Bool(true));
                i += 1;
            }
        } else {
            i += 1;
        }
    }
    (name, map)
}

fn main() {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "--mcp") {
        lilyco::serve_mcp(build_registry());
        return;
    }
    if args.iter().any(|a| a == "--list") {
        println!("{}", build_registry().to_json());
        return;
    }
    if let Some(pos) = args.iter().position(|a| a == "--serve") {
        let port = args.get(pos + 1).and_then(|p| p.parse().ok()).unwrap_or(9901);
        serve(port);
        return;
    }

    let (name, map) = parse_cli(&args);
    let registry = build_registry();
    let Some(cmd) = registry.get(&name) else {
        eprintln!(
            "lly — Radxa 工具包\n用法:\n  lly --mcp\n  lly --list\n  lly --serve 9901\n  lly tts --text ... [--voice ... --rate ...]\n  lly transcribe --audio <file> [--language zh --model base]\n  lly file --path <file> --action info|safe|convert|parse [--format mp3|xlsx|toml|json] [--args ...]\n可用工具: {:?}",
            registry.names()
        );
        std::process::exit(2);
    };
    let Some(handler) = &cmd.handler else {
        eprintln!("{name} 无 handler");
        std::process::exit(2);
    };
    let args_val = serde_json::Value::Object(map.into_iter().collect());
    let (tx, _rx) = std::sync::mpsc::channel();
    let ctx = Context::new_test(tx);
    match handler(&ctx, &args_val) {
        Ok(v) => {
            println!("{}", serde_json::to_string_pretty(&v).unwrap_or_default());
        }
        Err(e) => {
            eprintln!("错误: {e}");
            std::process::exit(1);
        }
    }
}
