//! Edge TTS —— 基于 msedge-tts crate（25k 下载实战检验）。
//!
//! 自研协议层（Sec-MS-GEC / WSS 握手头 / X-Timestamp）已退役：
//! 采纳优于自修（信条 3），调研依据 docs/research-loop-20260912.md。
//! 保留原签名 synthesize(text, voice, rate, pitch) -> mp3 bytes，对 tools.rs 零改动。

use msedge_tts::tts::client::connect;
use msedge_tts::tts::SpeechConfig;

/// 合成语音，返回 mp3 二进制
pub fn synthesize(text: &str, voice: &str, rate: &str, pitch: &str) -> Result<Vec<u8>, String> {
    let config = SpeechConfig {
        voice_name: voice.to_string(),
        audio_format: "audio-24khz-48kbitrate-mono-mp3".to_string(),
        pitch: parse_offset(pitch),
        rate: parse_offset(rate),
        volume: 0,
    };
    let mut tts = connect().map_err(|e| format!("connect: {e}"))?;
    let audio = tts
        .synthesize(text, &config)
        .map_err(|e| format!("synthesize: {e}"))?;
    if audio.audio_bytes.is_empty() {
        return Err("empty audio (voice name 可能无效)".into());
    }
    Ok(audio.audio_bytes)
}

/// "+10%" / "-5Hz" / "10" → i32 偏移；容错解析，非法值回退 0
fn parse_offset(s: &str) -> i32 {
    s.trim()
        .trim_start_matches('+')
        .trim_end_matches('%')
        .trim_end_matches("Hz")
        .trim()
        .parse()
        .unwrap_or(0)
}
