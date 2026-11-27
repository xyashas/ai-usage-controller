//! Local on-device speech-to-text fallback using Whisper `tiny.en` (pure Rust via candle).
//!
//! Used automatically when Windows WinRT SpeechRecognizer returns 0 guesses/phrases
//! or fails to start (and on macOS). Everything runs 100% on-device: the model files
//! (~153 MB total) are downloaded once on first use into `<app_data>/whisper-tiny.en/`
//! with a live progress notice, and temp WAV files are deleted immediately after transcription.

use candle_core::{Device, IndexOp, Tensor};
use candle_nn::VarBuilder;
use candle_transformers::models::whisper::{self as m, audio, Config};
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use tokenizers::Tokenizer;

const HF_BASE: &str = "https://huggingface.co/openai/whisper-tiny.en/resolve/main";

struct Loaded {
    model: m::model::Whisper,
    config: Config,
    tokenizer: Tokenizer,
    mel_filters: Vec<f32>,
}

static ENGINE: OnceLock<Mutex<Option<Loaded>>> = OnceLock::new();

fn model_dir() -> PathBuf {
    aiuc_core::paths::app_data_dir().join("whisper-tiny.en")
}

/// Slaney-scale mel filterbank matching OpenAI Whisper (16 kHz, n_fft=400, n_mels=80).
pub fn slaney_mel_filters(sample_rate: usize, n_fft: usize, n_mels: usize) -> Vec<f32> {
    let n_freqs = n_fft / 2 + 1;
    let f_min = 0.0f64;
    let f_max = sample_rate as f64 / 2.0;

    let hz_to_mel = |f: f64| -> f64 {
        let f_sp = 200.0 / 3.0;
        let min_log_hz = 1000.0;
        let min_log_mel = min_log_hz / f_sp;
        let logstep = (6.4f64).ln() / 27.0;
        if f >= min_log_hz {
            min_log_mel + (f / min_log_hz).ln() / logstep
        } else {
            f / f_sp
        }
    };
    let mel_to_hz = |mel: f64| -> f64 {
        let f_sp = 200.0 / 3.0;
        let min_log_hz = 1000.0;
        let min_log_mel = min_log_hz / f_sp;
        let logstep = (6.4f64).ln() / 27.0;
        if mel >= min_log_mel {
            min_log_hz * (logstep * (mel - min_log_mel)).exp()
        } else {
            f_sp * mel
        }
    };

    let mel_min = hz_to_mel(f_min);
    let mel_max = hz_to_mel(f_max);
    let mut mel_pts = Vec::with_capacity(n_mels + 2);
    for i in 0..=(n_mels + 1) {
        mel_pts.push(mel_to_hz(mel_min + (mel_max - mel_min) * (i as f64) / ((n_mels + 1) as f64)));
    }
    let fft_freqs: Vec<f64> = (0..n_freqs)
        .map(|i| (i as f64) * (sample_rate as f64) / (n_fft as f64))
        .collect();

    let mut weights = vec![0.0f32; n_mels * n_freqs];
    for m_idx in 0..n_mels {
        let f_left = mel_pts[m_idx];
        let f_center = mel_pts[m_idx + 1];
        let f_right = mel_pts[m_idx + 2];
        let enorm = 2.0 / (f_right - f_left);
        for (k, &freq) in fft_freqs.iter().enumerate() {
            let lower = if f_center > f_left { (freq - f_left) / (f_center - f_left) } else { 0.0 };
            let upper = if f_right > f_center { (f_right - freq) / (f_right - f_center) } else { 0.0 };
            let val = lower.min(upper).max(0.0) * enorm;
            weights[m_idx * n_freqs + k] = val as f32;
        }
    }
    weights
}

fn download_file(url: &str, dest: &Path, report_progress: bool, on_status: &dyn Fn(&str)) -> Result<(), String> {
    if dest.is_file() && fs::metadata(dest).is_ok_and(|m| m.len() > 100) {
        return Ok(());
    }
    if let Some(p) = dest.parent() {
        fs::create_dir_all(p).map_err(|e| format!("Couldn't create speech model folder: {e}"))?;
    }
    let tmp = dest.with_extension("part");
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(std::time::Duration::from_secs(300)))
        .build()
        .into();
    let resp = agent
        .get(url)
        .call()
        .map_err(|e| format!("Couldn't download offline speech model: {e}"))?;
    let total: Option<u64> = resp
        .headers()
        .get("content-length")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.parse().ok());
    let mut reader = resp.into_body().into_reader();
    let mut out = File::create(&tmp).map_err(|e| format!("Couldn't write speech model: {e}"))?;
    let mut buf = [0u8; 64 * 1024];
    let mut got: u64 = 0;
    let mut last_pct: u64 = 101;
    loop {
        let n = reader.read(&mut buf).map_err(|e| format!("Speech model download interrupted: {e}"))?;
        if n == 0 {
            break;
        }
        out.write_all(&buf[..n]).map_err(|e| format!("Couldn't save speech model: {e}"))?;
        got += n as u64;
        if report_progress {
            if let Some(t) = total.filter(|&t| t > 0) {
                let pct = (got * 100 / t).min(100);
                if pct != last_pct && (pct.is_multiple_of(5) || pct == 100) {
                    last_pct = pct;
                    on_status(&format!("Downloading offline speech model ({pct}%)…"));
                }
            }
        }
    }
    drop(out);
    fs::rename(&tmp, dest).map_err(|e| format!("Couldn't finalize speech model: {e}"))?;
    Ok(())
}

fn ensure_loaded(on_status: &dyn Fn(&str)) -> Result<(), String> {
    let slot = ENGINE.get_or_init(|| Mutex::new(None));
    if slot.lock().map_err(|_| "Speech engine lock error".to_string())?.is_some() {
        return Ok(());
    }
    let dir = model_dir();
    let cfg_path = dir.join("config.json");
    let tok_path = dir.join("tokenizer.json");
    let weights_path = dir.join("model.safetensors");

    let need_dl = !weights_path.is_file() || !cfg_path.is_file() || !tok_path.is_file();
    if need_dl {
        on_status("Downloading offline speech model…");
        crate::ask::diag("voice whisper: downloading tiny.en model");
    }
    download_file(&format!("{HF_BASE}/config.json"), &cfg_path, false, on_status)?;
    download_file(&format!("{HF_BASE}/tokenizer.json"), &tok_path, false, on_status)?;
    download_file(&format!("{HF_BASE}/model.safetensors"), &weights_path, true, on_status)?;

    on_status("Loading offline speech model…");
    let config: Config = serde_json::from_str(
        &fs::read_to_string(&cfg_path).map_err(|e| format!("Bad speech config: {e}"))?,
    )
    .map_err(|e| format!("Bad speech config JSON: {e}"))?;
    let tokenizer = Tokenizer::from_file(&tok_path).map_err(|e| format!("Bad speech tokenizer: {e}"))?;
    let device = Device::Cpu;
    let vb = unsafe {
        VarBuilder::from_mmaped_safetensors(&[weights_path], m::DTYPE, &device)
            .map_err(|e| format!("Couldn't load speech weights: {e}"))?
    };
    let model = m::model::Whisper::load(&vb, config.clone())
        .map_err(|e| format!("Couldn't initialize Whisper model: {e}"))?;
    let mel_filters = slaney_mel_filters(m::SAMPLE_RATE, m::N_FFT, config.num_mel_bins);
    if let Ok(mut guard) = slot.lock() {
        *guard = Some(Loaded { model, config, tokenizer, mel_filters });
    }
    crate::ask::diag("voice whisper: tiny.en model loaded");
    Ok(())
}

/// Transcribe a 16 kHz mono 16-bit PCM WAV file on-device using Whisper `tiny.en`.
pub fn transcribe_wav(wav_path: &Path, on_status: &dyn Fn(&str)) -> Result<String, String> {
    let t0 = std::time::Instant::now();
    ensure_loaded(on_status)?;
    on_status("Transcribing…");

    let mut reader = hound::WavReader::open(wav_path).map_err(|e| format!("Couldn't read audio: {e}"))?;
    let mut pcm: Vec<f32> = reader
        .samples::<i16>()
        .map(|s| s.unwrap_or(0) as f32 / 32768.0)
        .collect();
    if pcm.is_empty() {
        return Ok(String::new());
    }
    // Pad or trim to 30 seconds (Whisper's fixed input window)
    if pcm.len() > m::N_SAMPLES {
        pcm.truncate(m::N_SAMPLES);
    } else if pcm.len() < m::N_SAMPLES {
        pcm.resize(m::N_SAMPLES, 0.0);
    }

    let slot = ENGINE.get_or_init(|| Mutex::new(None));
    let mut guard = slot.lock().map_err(|_| "Speech engine lock error".to_string())?;
    let loaded = guard.as_mut().ok_or_else(|| "Speech model not loaded".to_string())?;

    let mel = audio::pcm_to_mel(&loaded.config, &pcm, &loaded.mel_filters);
    let mel_len = mel.len();
    let device = Device::Cpu;
    let mel_tensor = Tensor::from_vec(
        mel,
        (1, loaded.config.num_mel_bins, mel_len / loaded.config.num_mel_bins),
        &device,
    )
    .map_err(|e| format!("Mel tensor error: {e}"))?;

    loaded.model.reset_kv_cache();
    let audio_features = loaded
        .model
        .encoder
        .forward(&mel_tensor, true)
        .map_err(|e| format!("Whisper encode error: {e}"))?;

    let sot = loaded.tokenizer.token_to_id(m::SOT_TOKEN).unwrap_or(50257);
    let no_ts = loaded.tokenizer.token_to_id(m::NO_TIMESTAMPS_TOKEN).unwrap_or(50362);
    let eot = loaded.tokenizer.token_to_id(m::EOT_TOKEN).unwrap_or(50256);

    let mut tokens = vec![sot, no_ts];
    let max_tokens = 224usize;
    for i in 0..max_tokens {
        let input = if i == 0 {
            Tensor::new(tokens.as_slice(), &device)
        } else {
            Tensor::new(&[*tokens.last().unwrap_or(&eot)], &device)
        }
        .map_err(|e| format!("Token tensor error: {e}"))?
        .unsqueeze(0)
        .map_err(|e| format!("Unsqueeze error: {e}"))?;

        let ys = loaded
            .model
            .decoder
            .forward(&input, &audio_features, i == 0)
            .map_err(|e| format!("Whisper decode error: {e}"))?;
        let (_, seq_len, _) = ys.dims3().map_err(|e| format!("Dims error: {e}"))?;
        let logits = loaded
            .model
            .decoder
            .final_linear(&ys.i((..1, seq_len - 1..))
            .map_err(|e| format!("Slice error: {e}"))?)
            .map_err(|e| format!("Linear error: {e}"))?
            .i(0)
            .and_then(|t| t.i(0))
            .map_err(|e| format!("Logits index error: {e}"))?;
        let next = logits
            .argmax(0)
            .and_then(|t| t.to_scalar::<u32>())
            .map_err(|e| format!("Argmax error: {e}"))?;
        if next == eot {
            break;
        }
        tokens.push(next);
    }

    let text = loaded
        .tokenizer
        .decode(&tokens[2..], true)
        .map_err(|e| format!("Decode error: {e}"))?;
    let cleaned = text.trim().to_owned();
    crate::ask::diag(&format!(
        "voice whisper: {:.2}s words={}",
        t0.elapsed().as_secs_f32(),
        cleaned.split_whitespace().count()
    ));
    Ok(cleaned)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slaney_mel_filterbank_has_expected_dimensions_and_non_negative_weights() {
        let w = slaney_mel_filters(16_000, 400, 80);
        assert_eq!(w.len(), 80 * 201);
        assert!(w.iter().all(|&x| x >= 0.0 && x.is_finite()));
        assert!(w.iter().any(|&x| x > 0.0));
    }
}
