//! Облачный синтез речи: ElevenLabs и Google Cloud TTS.
//! Аудио получается по HTTP, пишется во временный файл и проигрывается системным
//! плеером — без нативных аудио-зависимостей.

use base64::Engine;
use whatcode_core::config::{TtsProvider, VoiceConfig};
use serde_json::json;
use std::process::Stdio;
use std::time::Duration;

/// Синтезировать речь и проиграть её. Возвращает текстовую ошибку при сбое.
pub async fn synthesize_and_play(
    http: &reqwest::Client,
    cfg: &VoiceConfig,
    provider: TtsProvider,
    text: &str,
) -> Result<(), String> {
    let (bytes, ext) = match provider {
        TtsProvider::ElevenLabs => (elevenlabs(http, cfg, text).await?, "mp3"),
        TtsProvider::GoogleCloud => (google(http, cfg, text).await?, "mp3"),
        TtsProvider::Azure => (azure(http, cfg, text).await?, "mp3"),
        TtsProvider::Qwen => (qwen(http, cfg, text).await?, "mp3"),
        TtsProvider::Edge => (edge(cfg, text).await?, "mp3"),
        TtsProvider::System => return Err("system-провайдер не использует облако".into()),
    };
    let path = write_temp(&bytes, ext)?;
    play(&path)
}

async fn elevenlabs(
    http: &reqwest::Client,
    cfg: &VoiceConfig,
    text: &str,
) -> Result<Vec<u8>, String> {
    let key = cfg
        .elevenlabs_api_key
        .as_deref()
        .ok_or("нет ELEVENLABS_API_KEY")?;
    let voice_id = cfg
        .elevenlabs_voice_id
        .as_deref()
        .unwrap_or("ZYcSL3av41fQqtckDugo");
    let model = cfg
        .elevenlabs_model
        .as_deref()
        .unwrap_or("eleven_multilingual_v2");
    let url = format!("https://api.elevenlabs.io/v1/text-to-speech/{voice_id}");
    let resp = http
        .post(&url)
        .header("xi-api-key", key)
        .header("accept", "audio/mpeg")
        .json(&json!({ "text": text, "model_id": model }))
        .timeout(Duration::from_secs(60))
        .send()
        .await
        .map_err(|e| format!("сеть ElevenLabs: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("ElevenLabs HTTP {}", resp.status().as_u16()));
    }
    Ok(resp.bytes().await.map_err(|e| e.to_string())?.to_vec())
}

async fn google(http: &reqwest::Client, cfg: &VoiceConfig, text: &str) -> Result<Vec<u8>, String> {
    let key = cfg
        .google_api_key
        .as_deref()
        .ok_or("нет GOOGLE_TTS_API_KEY")?;
    let language = cfg.google_language.as_deref().unwrap_or("ru-RU");
    let voice = cfg.google_voice.as_deref().unwrap_or("ru-RU-Standard-A");
    let url = format!("https://texttospeech.googleapis.com/v1/text:synthesize?key={key}");
    let resp = http
        .post(&url)
        .json(&json!({
            "input": { "text": text },
            "voice": { "languageCode": language, "name": voice },
            "audioConfig": { "audioEncoding": "MP3" }
        }))
        .timeout(Duration::from_secs(60))
        .send()
        .await
        .map_err(|e| format!("сеть Google TTS: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("Google TTS HTTP {}", resp.status().as_u16()));
    }
    let value: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
    let b64 = value
        .get("audioContent")
        .and_then(|v| v.as_str())
        .ok_or("нет audioContent в ответе")?;
    base64::engine::general_purpose::STANDARD
        .decode(b64)
        .map_err(|e| format!("base64: {e}"))
}

/// Microsoft Azure Speech: SSML → MP3 через REST cognitiveservices.
async fn azure(http: &reqwest::Client, cfg: &VoiceConfig, text: &str) -> Result<Vec<u8>, String> {
    let key = cfg
        .azure_api_key
        .as_deref()
        .ok_or("нет AZURE_TTS_API_KEY")?;
    let region = cfg.azure_region.as_deref().ok_or("нет AZURE_TTS_REGION")?;
    let voice = cfg.azure_voice.as_deref().unwrap_or("ru-RU-SvetlanaNeural");
    let lang = voice.split('-').take(2).collect::<Vec<_>>().join("-");
    let url = format!("https://{region}.tts.speech.microsoft.com/cognitiveservices/v1");
    // Экранируем XML-спецсимволы в тексте для SSML.
    let safe = text
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;");
    let ssml = format!(
        "<speak version='1.0' xml:lang='{lang}'><voice xml:lang='{lang}' name='{voice}'>{safe}</voice></speak>"
    );
    let resp = http
        .post(&url)
        .header("Ocp-Apim-Subscription-Key", key)
        .header("Content-Type", "application/ssml+xml")
        .header(
            "X-Microsoft-OutputFormat",
            "audio-24khz-48kbitrate-mono-mp3",
        )
        .header("User-Agent", "WhatCode")
        .body(ssml)
        .timeout(Duration::from_secs(60))
        .send()
        .await
        .map_err(|e| format!("сеть Azure TTS: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("Azure TTS HTTP {}", resp.status().as_u16()));
    }
    Ok(resp.bytes().await.map_err(|e| e.to_string())?.to_vec())
}

/// Microsoft Edge TTS через `edge-tts` CLI.
async fn edge(cfg: &VoiceConfig, text: &str) -> Result<Vec<u8>, String> {
    let out = write_temp(&[], "mp3")?;
    let out_str = out.to_string_lossy().to_string();
    let voice = cfg.edge_voice.as_deref().unwrap_or("ru-RU-SvetlanaNeural");
    let mut args = vec![
        "--text".to_string(),
        text.to_string(),
        "--voice".to_string(),
        voice.to_string(),
        "--write-media".to_string(),
        out_str,
    ];
    if let Some(rate) = &cfg.edge_rate {
        args.push("--rate".to_string());
        args.push(rate.clone());
    }
    if let Some(volume) = &cfg.edge_volume {
        args.push("--volume".to_string());
        args.push(volume.clone());
    }
    if let Some(pitch) = &cfg.edge_pitch {
        args.push("--pitch".to_string());
        args.push(pitch.clone());
    }

    let status = tokio::process::Command::new("edge-tts")
        .args(&args)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .await
        .map_err(|e| format!("не удалось запустить `edge-tts`: {e}. Установите: uv pip install edge-tts"))?;
    if !status.success() {
        return Err("`edge-tts` завершился с ошибкой".into());
    }
    tokio::fs::read(&out)
        .await
        .map_err(|e| format!("не удалось прочитать результат edge-tts: {e}"))
}

/// Alibaba Qwen / DashScope TTS (международный эндпоинт по умолчанию).
async fn qwen(http: &reqwest::Client, cfg: &VoiceConfig, text: &str) -> Result<Vec<u8>, String> {
    let key = cfg.qwen_api_key.as_deref().ok_or("нет QWEN_TTS_API_KEY")?;
    let model = cfg.qwen_model.as_deref().unwrap_or("qwen-tts");
    let voice = cfg.qwen_voice.as_deref().unwrap_or("Chelsie");
    let base = cfg
        .qwen_base_url
        .as_deref()
        .unwrap_or("https://dashscope-intl.aliyuncs.com/api/v1");
    let url = format!(
        "{}/services/aigc/multimodal-generation/generation",
        base.trim_end_matches('/')
    );
    let resp = http
        .post(&url)
        .bearer_auth(key)
        .json(&json!({
            "model": model,
            "input": { "text": text, "voice": voice },
        }))
        .timeout(Duration::from_secs(60))
        .send()
        .await
        .map_err(|e| format!("сеть Qwen TTS: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("Qwen TTS HTTP {}", resp.status().as_u16()));
    }
    // DashScope возвращает ссылку на аудио в output.audio.url — скачиваем её.
    let value: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
    let audio_url = value
        .pointer("/output/audio/url")
        .and_then(|v| v.as_str())
        .ok_or("нет output.audio.url в ответе Qwen")?;
    let audio = http
        .get(audio_url)
        .timeout(Duration::from_secs(60))
        .send()
        .await
        .map_err(|e| format!("скачивание Qwen-аудио: {e}"))?;
    Ok(audio.bytes().await.map_err(|e| e.to_string())?.to_vec())
}

fn write_temp(bytes: &[u8], ext: &str) -> Result<std::path::PathBuf, String> {
    let mut path = std::env::temp_dir();
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    path.push(format!("whatcode-tts-{nanos}.{ext}"));
    std::fs::write(&path, bytes).map_err(|e| format!("запись temp: {e}"))?;
    Ok(path)
}

/// Проиграть аудиофайл доступным системным плеером.
fn play(path: &std::path::Path) -> Result<(), String> {
    let p = path.to_string_lossy().to_string();

    #[cfg(target_os = "macos")]
    let candidates: Vec<(&str, Vec<String>)> = vec![("afplay", vec![p.clone()])];

    #[cfg(target_os = "windows")]
    let candidates: Vec<(&str, Vec<String>)> = vec![(
        "powershell",
        vec![
            "-NoProfile".into(),
            "-Command".into(),
            format!("(New-Object Media.SoundPlayer '{p}').PlaySync()"),
        ],
    )];

    #[cfg(all(unix, not(target_os = "macos")))]
    let candidates: Vec<(&str, Vec<String>)> = vec![
        (
            "ffplay",
            vec![
                "-nodisp".into(),
                "-autoexit".into(),
                "-loglevel".into(),
                "quiet".into(),
                p.clone(),
            ],
        ),
        (
            "mpv",
            vec!["--no-video".into(), "--really-quiet".into(), p.clone()],
        ),
        ("mpg123", vec!["-q".into(), p.clone()]),
        (
            "cvlc",
            vec![
                "--play-and-exit".into(),
                "--intf".into(),
                "dummy".into(),
                p.clone(),
            ],
        ),
    ];

    for (player, args) in &candidates {
        if crate::which(player) {
            let spawned = std::process::Command::new(player)
                .args(args)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn();
            if spawned.is_ok() {
                return Ok(());
            }
        }
    }
    Err("не найден аудиоплеер (ffplay/mpv/afplay/...)".into())
}
