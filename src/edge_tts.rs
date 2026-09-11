//! Edge TTS — 微软 Edge 在线语音合成（Rust 实现）
//!
//! 协议要点（参考 rany2/edge-tts 11800★ Python 实现）：
//! - WSS: speech.platform.bing.com .../edge/v1?TrustedClientToken=...&ConnectionId=...&Sec-MS-GEC=...&Sec-MS-GEC-Version=...
//! - Sec-MS-GEC = SHA256("{windows_filetime_ticks(5min取整)}{TrustedClientToken}") 大写 hex
//! - 消息流：Path:speech.config (JSON) → Path:ssml (X-RequestId 配对) → Path:audio 二进制体 → Path:turn.end

use sha2::{Digest, Sha256};

const TRUSTED_CLIENT_TOKEN: &str = "6A5AA1D4EAFF4E9FB37E23D68491D6F4";
const WSS_BASE: &str =
    "wss://speech.platform.bing.com/consumer/speech/synthesize/readaloud/edge/v1";
const CHROMIUM_FULL_VERSION: &str = "143.0.3650.75";
const WIN_EPOCH_SECS: f64 = 11_644_473_600.0; // 1601-01-01 → 1970-01-01 偏移

/// 生成 Sec-MS-GEC token（当前时间 → Windows file time → 5 分钟取整 → SHA256 hex 大写）
fn sec_ms_gec() -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock before epoch")
        .as_secs_f64();
    let ticks = now + WIN_EPOCH_SECS;
    let ticks = ticks - ticks % 300.0;
    let ticks = (ticks * 10_000_000.0).round() as u64;
    let mut hasher = Sha256::new();
    hasher.update(format!("{ticks}{TRUSTED_CLIENT_TOKEN}").as_bytes());
    let digest = hasher.finalize();
    digest.iter().map(|b| format!("{b:02X}")).collect()
}

/// 随机 32 位 hex（ConnectionId / X-RequestId）
fn random_id() -> String {
    use rand::Rng;
    let mut rng = rand::thread_rng();
    (0..16).map(|_| format!("{:02x}", rng.gen::<u8>())).collect()
}

/// XML 转义（文本内容）
fn escape_pcdata(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// JS 风格日期串：%a %b %d %Y %H:%M:%S GMT+0000 (Coordinated Universal Time)
fn date_to_string() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_secs();
    let days = secs / 86400;
    let rem = secs % 86400;
    let (h, mi, s) = (rem / 3600, (rem % 3600) / 60, rem % 60);
    // 1970-01-01 是周四(4)。civil 换算用 Howard Hinnant 算法
    let z = days as i64 + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    let wd = ((days + 4) % 7) as usize; // 1970-01-01=周四
    const WD: [&str; 7] = ["Thu", "Fri", "Sat", "Sun", "Mon", "Tue", "Wed"];
    const MON: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    format!(
        "{} {} {:02} {} {:02}:{:02}:{:02} GMT+0000 (Coordinated Universal Time)",
        WD[wd], MON[(m - 1) as usize], d, y, h, mi, s
    )
}

/// 合成语音，返回 mp3 二进制
pub fn synthesize(text: &str, voice: &str, rate: &str, pitch: &str) -> Result<Vec<u8>, String> {
    let mut url = url::Url::parse(WSS_BASE).map_err(|e| format!("url: {e}"))?;
    let conn_id = random_id();
    let gec = sec_ms_gec();
    {
        let mut q = url.query_pairs_mut();
        q.append_pair("TrustedClientToken", TRUSTED_CLIENT_TOKEN);
        q.append_pair("ConnectionId", &conn_id);
        q.append_pair("Sec-MS-GEC", &gec);
        q.append_pair("Sec-MS-GEC-Version", &format!("1-{CHROMIUM_FULL_VERSION}"));
    }
    // 与 rany2/edge-tts 一致的浏览器头（缺 User-Agent / Origin 会 403）
    let uri: tungstenite::http::Uri = url
        .as_str()
        .parse()
        .map_err(|e| format!("uri: {e}"))?;
    let request = tungstenite::client::ClientRequestBuilder::new(uri)
        .with_header("User-Agent", format!("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/143.0.0.0 Safari/537.36 Edg/143.0.0.0"))
        .with_header("Origin", "chrome-extension://jdiccldimpdaibmpdkjnbmckianbfold")
        .with_header("Pragma", "no-cache")
        .with_header("Cache-Control", "no-cache")
        .with_header("Sec-MS-GEC", &gec)
        .with_header("Sec-MS-GEC-Version", &format!("1-{CHROMIUM_FULL_VERSION}"))
        .with_header("Accept-Encoding", "gzip, deflate, br, zstd")
        .with_header("Accept-Language", "en-US,en;q=0.9");
    let (mut socket, _) = tungstenite::connect(request).map_err(|e| format!("connect: {e}"))?;

    // 1) speech.config
    let cfg = format!(
        "X-Timestamp:{}\r\nContent-Type:application/json; charset=utf-8\r\nPath:speech.config\r\n\r\n\
         {{\"context\":{{\"synthesis\":{{\"audio\":{{\"metadataoptions\":{{\"sentenceBoundaryEnabled\":\"false\",\
         \"wordBoundaryEnabled\":\"true\"}},\"outputFormat\":\"audio-24khz-48kbitrate-mono-mp3\"}}}}}}}}\r\n",
        date_to_string()
    );
    socket
        .send(tungstenite::Message::Text(cfg.into()))
        .map_err(|e| format!("send config: {e}"))?;

    // 2) ssml
    let ssml = format!(
        "<speak version=\"1.0\" xmlns=\"http://www.w3.org/2001/10/synthesis\" \
         xmlns:mstts=\"https://www.w3.org/2001/mstts\" xml:lang=\"en-US\">\
         <voice name=\"{voice}\"><prosody pitch=\"{pitch}\" rate=\"{rate}\" volume=\"+0%\">\
         {text}</prosody></voice></speak>",
        voice = voice,
        pitch = pitch,
        rate = rate,
        text = escape_pcdata(text),
    );
    let request_id = random_id();
    socket
        .send(tungstenite::Message::Text(
            format!(
                "X-RequestId:{request_id}\r\nContent-Type:application/ssml+xml\r\nX-Timestamp:{}Z\r\nPath:ssml\r\n\r\n{ssml}",
                date_to_string()
            )
            .into(),
        ))
        .map_err(|e| format!("send ssml: {e}"))?;

    // 3) 收集音频直到 turn.end
    let mut buf: Vec<u8> = Vec::new();
    loop {
        match socket.read() {
            Ok(tungstenite::Message::Text(s)) => {
                if s.contains("Path:turn.end") {
                    break;
                }
            }
            Ok(tungstenite::Message::Binary(s)) => {
                if s.len() >= 2 {
                    let header_len = (s[0] as usize) * 256 + s[1] as usize;
                    if s.len() >= header_len + 2 {
                        buf.extend_from_slice(&s[header_len + 2..]);
                    }
                }
            }
            Ok(_) => {}
            Err(e) => return Err(format!("read: {e}")),
        }
    }
    if buf.is_empty() {
        return Err("empty audio (voice name 可能无效)".into());
    }
    Ok(buf)
}
