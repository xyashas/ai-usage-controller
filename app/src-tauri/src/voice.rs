//! Hold-to-talk: listen while the Ask hotkey is held.
//!
//! On Windows, runs WinRT SpeechRecognizer (for instant live captions when available)
//! alongside a direct `cpal` (WASAPI) microphone recorder to a temporary 16 kHz WAV.
//! If WinRT fails to start or returns 0 guesses/phrases, automatically falls back to
//! local on-device Whisper `tiny.en` transcription using the audio already captured.
//! On macOS, uses `cpal` (CoreAudio) + local Whisper `tiny.en`.
//! Audio stays 100% on-device and temp WAV files are deleted immediately after use.

#[cfg(any(windows, target_os = "macos"))]
use std::path::PathBuf;
#[cfg(any(windows, target_os = "macos"))]
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};
#[cfg(windows)]
use std::sync::Once;

type StopFn = Box<dyn FnOnce(&dyn Fn(&str)) -> Result<String, String> + Send>;

pub struct Listening {
    stop: StopFn,
}

impl Listening {
    /// Stop listening and return what was said. `on_status` receives progress updates
    /// if the local Whisper fallback downloads or transcribes.
    pub fn finish(self, on_status: impl Fn(&str)) -> Result<String, String> {
        (self.stop)(&on_status)
    }
}

// ---------- Direct mic capture via cpal (Windows WASAPI / macOS CoreAudio) ----------

#[cfg(any(windows, target_os = "macos"))]
struct MicCapture {
    wav_path: PathBuf,
    stop: Box<dyn FnOnce() -> (u64, f32) + Send>,
}

#[cfg(any(windows, target_os = "macos"))]
fn start_cpal_capture() -> Result<MicCapture, String> {
    use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

    let host = cpal::default_host();
    let device = host
        .default_input_device()
        .ok_or_else(|| "No microphone found. Plug one in or check Settings → System → Sound.".to_string())?;
    let cfg = device
        .default_input_config()
        .map_err(|e| format!("Couldn't open microphone: {e}"))?;
    let in_rate = cfg.sample_rate();
    let channels = cfg.channels() as usize;
    let sample_format = cfg.sample_format();

    let wav_path = crate::capture::temp_dir()
        .join(format!("voice-{}.wav", chrono::Utc::now().timestamp_millis()));
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: 16_000,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let writer = hound::WavWriter::create(&wav_path, spec)
        .map_err(|e| format!("Couldn't create temp audio file: {e}"))?;

    struct RecState {
        writer: Option<hound::WavWriter<std::io::BufWriter<std::fs::File>>>,
        phase: f64,
        step: f64,
        samples_written: u64,
        peak: f32,
    }
    let state = Arc::new(Mutex::new(RecState {
        writer: Some(writer),
        phase: 0.0,
        step: 16_000.0 / (in_rate.max(1) as f64),
        samples_written: 0,
        peak: 0.0,
    }));
    let active = Arc::new(AtomicBool::new(true));

    let push_mono = |st: &Arc<Mutex<RecState>>, act: &Arc<AtomicBool>, mono: &[f32]| {
        if !act.load(Ordering::Relaxed) {
            return;
        }
        let Ok(mut g) = st.lock() else { return };
        let step = g.step;
        let mut phase = g.phase;
        let mut peak = g.peak;
        let mut written = 0u64;
        if let Some(w) = g.writer.as_mut() {
            for &s in mono {
                let a = s.abs();
                if a > peak {
                    peak = a;
                }
                phase += step;
                while phase >= 1.0 {
                    phase -= 1.0;
                    let i = (s.clamp(-1.0, 1.0) * 32767.0) as i16;
                    let _ = w.write_sample(i);
                    written += 1;
                }
            }
        }
        g.phase = phase;
        g.peak = peak;
        g.samples_written += written;
    };

    let err_fn = |err| {
        crate::ask::diag(&format!("voice cpal stream err: {err}"));
    };
    let stream_cfg: cpal::StreamConfig = cfg.into();
    let (st, act) = (state.clone(), active.clone());
    let stream = match sample_format {
        cpal::SampleFormat::F32 => device.build_input_stream(
            stream_cfg,
            move |data: &[f32], _| {
                let mono: Vec<f32> = data
                    .chunks(channels.max(1))
                    .map(|f| f.iter().sum::<f32>() / f.len().max(1) as f32)
                    .collect();
                push_mono(&st, &act, &mono);
            },
            err_fn,
            None,
        ),
        cpal::SampleFormat::I16 => device.build_input_stream(
            stream_cfg,
            move |data: &[i16], _| {
                let mono: Vec<f32> = data
                    .chunks(channels.max(1))
                    .map(|f| f.iter().map(|&x| x as f32 / 32768.0).sum::<f32>() / f.len().max(1) as f32)
                    .collect();
                push_mono(&st, &act, &mono);
            },
            err_fn,
            None,
        ),
        cpal::SampleFormat::U16 => device.build_input_stream(
            stream_cfg,
            move |data: &[u16], _| {
                let mono: Vec<f32> = data
                    .chunks(channels.max(1))
                    .map(|f| f.iter().map(|&x| (x as f32 - 32768.0) / 32768.0).sum::<f32>() / f.len().max(1) as f32)
                    .collect();
                push_mono(&st, &act, &mono);
            },
            err_fn,
            None,
        ),
        other => return Err(format!("Unsupported microphone format: {other:?}")),
    }
    .map_err(|e| format!("Couldn't start microphone stream: {e}"))?;

    stream.play().map_err(|e| format!("Couldn't record from microphone: {e}"))?;

    // Keep the cpal::Stream alive on a dedicated thread until stopped (Stream is !Send on some OSes,
    // wait: on Windows WASAPI/macOS CoreAudio we drop it via channel to keep MicCapture Send).
    let (stop_tx, stop_rx) = std::sync::mpsc::channel::<()>();
    let (done_tx, done_rx) = std::sync::mpsc::channel::<()>();
    let act_stop = active.clone();
    // Wrap stream in a holder thread? Wait: stream must be created on the thread if !Send.
    // Let's keep it simple: drop stream on finish by moving only if Send; if we build on a thread,
    // it works identically on every OS! Let's check below — cpal::Stream is Send on Windows & macOS,
    // or we can drop it cleanly via std::thread::spawn.
    let _ = (stop_tx, stop_rx, done_tx, done_rx);
    let state_fin = state.clone();
    Ok(MicCapture {
        wav_path,
        stop: Box::new(move || {
            act_stop.store(false, Ordering::SeqCst);
            drop(stream);
            if let Ok(mut g) = state_fin.lock() {
                if let Some(w) = g.writer.take() {
                    let _ = w.finalize();
                }
                let secs = g.samples_written as f32 / 16_000.0;
                crate::ask::diag(&format!(
                    "voice cpal: {:.1}s samples={} peak={:.3}",
                    secs, g.samples_written, g.peak
                ));
                (g.samples_written, g.peak)
            } else {
                (0, 0.0)
            }
        }),
    })
}

// ---------- Windows WinRT SpeechRecognizer (live captions) + cpal/Whisper fallback ----------

#[cfg(windows)]
fn prime_audio_capture() {
    static PRIMED: Once = Once::new();
    PRIMED.call_once(|| {
        use windows::Media::Capture::{MediaCapture, MediaCaptureInitializationSettings, StreamingCaptureMode};
        let res = (|| -> windows::core::Result<()> {
            let settings = MediaCaptureInitializationSettings::new()?;
            settings.SetStreamingCaptureMode(StreamingCaptureMode::Audio)?;
            let capture = MediaCapture::new()?;
            capture.InitializeWithSettingsAsync(&settings)?.get()?;
            let _ = capture.Close();
            Ok(())
        })();
        match res {
            Ok(()) => crate::ask::diag("voice prime: MediaCapture audio ok"),
            Err(e) => crate::ask::diag(&format!(
                "voice prime: MediaCapture audio err=0x{:08X} ({})",
                e.code().0 as u32,
                e.message()
            )),
        }
    });
}

#[cfg(windows)]
struct WinRtSession {
    stop: Box<dyn FnOnce() -> (String, u32, u32) + Send>,
}

#[cfg(windows)]
fn start_winrt(live: Arc<dyn Fn(String) + Send + Sync>) -> Result<WinRtSession, String> {
    use windows::core::HSTRING;
    use windows::Foundation::{TimeSpan, TypedEventHandler};
    use windows::Media::SpeechRecognition::{
        SpeechContinuousRecognitionCompletedEventArgs,
        SpeechContinuousRecognitionResultGeneratedEventArgs,
        SpeechContinuousRecognitionSession,
        SpeechRecognitionHypothesisGeneratedEventArgs,
        SpeechRecognitionResultStatus,
        SpeechRecognitionScenario,
        SpeechRecognitionTopicConstraint,
        SpeechRecognizer,
    };
    let text = Arc::new(Mutex::new(Vec::<String>::new()));
    let guess = Arc::new(Mutex::new(String::new()));
    let counts = Arc::new(Mutex::new((0u32, 0u32)));
    let (guess_w, counts_w) = (guess.clone(), counts.clone());
    let (stop_tx, stop_rx) = std::sync::mpsc::channel::<()>();
    let (ready_tx, ready_rx) = std::sync::mpsc::channel::<Result<(), String>>();
    let (done_tx, done_rx) = std::sync::mpsc::channel::<()>();
    let collected = text.clone();
    std::thread::spawn(move || {
        unsafe {
            let _ = windows::Win32::System::Com::CoInitializeEx(
                None,
                windows::Win32::System::Com::COINIT_MULTITHREADED,
            );
        }
        prime_audio_capture();
        let run = || -> windows::core::Result<SpeechRecognizer> {
            let rec = match SpeechRecognizer::SystemSpeechLanguage() {
                Ok(lang) => {
                    let tag = lang.LanguageTag().map(|s| s.to_string()).unwrap_or_else(|_| "unknown".into());
                    match SpeechRecognizer::Create(&lang) {
                        Ok(r) => {
                            crate::ask::diag(&format!("voice lang: {tag} (SystemSpeechLanguage)"));
                            r
                        }
                        Err(_) => SpeechRecognizer::new()?,
                    }
                }
                Err(_) => SpeechRecognizer::new()?,
            };
            if let Ok(constraints) = rec.Constraints() {
                if let Ok(dictation) = SpeechRecognitionTopicConstraint::Create(
                    SpeechRecognitionScenario::Dictation,
                    &HSTRING::from("dictation"),
                ) {
                    let _ = constraints.Append(&dictation);
                }
            }
            if let Ok(timeouts) = rec.Timeouts() {
                let _ = timeouts.SetInitialSilenceTimeout(TimeSpan { Duration: 10 * 10_000_000 });
                let _ = timeouts.SetEndSilenceTimeout(TimeSpan { Duration: 12 * 1_000_000 });
            }
            let compiled = rec.CompileConstraintsAsync()?.get()?;
            let status = compiled.Status()?;
            if status != SpeechRecognitionResultStatus::Success {
                crate::ask::diag(&format!("voice compile failed: status={status:?}"));
                return Err(windows::core::Error::new(
                    windows::core::HRESULT(0x80045509u32 as i32),
                    "speech setup failed",
                ));
            }
            let session = rec.ContinuousRecognitionSession()?;
            let _ = session.Completed(&TypedEventHandler::<
                SpeechContinuousRecognitionSession,
                SpeechContinuousRecognitionCompletedEventArgs,
            >::new(move |_, args| {
                if let Ok(a) = args.ok() {
                    if let Ok(st) = a.Status() {
                        crate::ask::diag(&format!("voice session completed: status={st:?}"));
                    }
                }
                Ok(())
            }));
            let (sink, live2, g, n) = (collected.clone(), live.clone(), guess_w.clone(), counts_w.clone());
            rec.HypothesisGenerated(&TypedEventHandler::<
                SpeechRecognizer,
                SpeechRecognitionHypothesisGeneratedEventArgs,
            >::new(move |_, args| {
                if let Ok(a) = args.ok() {
                    if let Ok(t) = a.Hypothesis().and_then(|h| h.Text()) {
                        let done = sink.lock().map(|v| v.join(" ")).unwrap_or_default();
                        let all = format!("{done} {t}").trim().to_owned();
                        if let Ok(mut gg) = g.lock() { gg.clone_from(&all); }
                        if let Ok(mut c) = n.lock() { c.0 += 1; }
                        live2(all);
                    }
                }
                Ok(())
            }))?;
            let (sink, live3, g, n) = (collected.clone(), live.clone(), guess_w.clone(), counts_w.clone());
            session.ResultGenerated(&TypedEventHandler::<
                SpeechContinuousRecognitionSession,
                SpeechContinuousRecognitionResultGeneratedEventArgs,
            >::new(move |_, args| {
                if let Ok(a) = args.ok() {
                    if let Ok(t) = a.Result().and_then(|r| r.Text()) {
                        let t = t.to_string();
                        if let Ok(mut c) = n.lock() { c.1 += 1; }
                        if !t.trim().is_empty() {
                            if let Ok(mut v) = sink.lock() {
                                v.push(t);
                                let all = v.join(" ");
                                if let Ok(mut gg) = g.lock() { gg.clone_from(&all); }
                                live3(all);
                            }
                        }
                    }
                }
                Ok(())
            }))?;
            session.StartAsync()?.get()?;
            Ok(rec)
        };
        let rec = match run() {
            Ok(r) => { let _ = ready_tx.send(Ok(())); r }
            Err(e) => {
                crate::ask::diag(&format!(
                    "voice winrt init err=0x{:08X} ({})",
                    e.code().0 as u32,
                    e.message()
                ));
                let _ = ready_tx.send(Err(e.message()));
                return;
            }
        };
        let _ = stop_rx.recv();
        if let Ok(s) = rec.ContinuousRecognitionSession() { let _ = s.StopAsync().and_then(|a| a.get()); }
        let _ = done_tx.send(());
    });
    ready_rx.recv().map_err(|_| "WinRT speech didn't start".to_string())??;
    Ok(WinRtSession {
        stop: Box::new(move || {
            let _ = stop_tx.send(());
            let _ = done_rx.recv_timeout(std::time::Duration::from_secs(3));
            let finals = text.lock().map(|v| v.join(" ")).unwrap_or_default();
            let last_guess = guess.lock().map(|g| g.clone()).unwrap_or_default();
            let said = if last_guess.trim().len() > finals.trim().len() { last_guess } else { finals };
            let (g, f) = counts.lock().map(|c| *c).unwrap_or((0, 0));
            crate::ask::diag(&format!(
                "voice heard: {g} guesses, {f} phrases, {} words",
                said.split_whitespace().count()
            ));
            (said.trim().to_owned(), g, f)
        }),
    })
}

/// Start listening. `live` gets the words so far as you speak (for captions).
#[cfg(windows)]
pub fn start(live: impl Fn(String) + Send + Sync + 'static) -> Result<Listening, String> {
    // 1. Start direct WASAPI mic capture via cpal so we ALWAYS have real audio even if
    //    Windows WinRT SpeechRecognizer fails or hears 0 guesses.
    let cpal_rec = start_cpal_capture();
    if let Err(ref e) = cpal_rec {
        crate::ask::diag(&format!("voice cpal start failed: {e}"));
    }
    // 2. Also start WinRT SpeechRecognizer for real-time captions when available.
    let live_arc: Arc<dyn Fn(String) + Send + Sync> = Arc::new(live);
    let winrt = start_winrt(live_arc).ok();
    if winrt.is_none() && cpal_rec.is_err() {
        return Err(cpal_rec.err().unwrap_or_else(|| "Couldn't start microphone.".into()));
    }
    let cpal_ok = cpal_rec.ok();
    Ok(Listening {
        stop: Box::new(move |on_status| {
            let winrt_said = winrt.map(|w| (w.stop)().0).unwrap_or_default();
            let cpal_result = cpal_ok.map(|m| {
                let wav = m.wav_path;
                let (samples, peak) = (m.stop)();
                (wav, samples, peak)
            });

            if !winrt_said.is_empty() {
                if let Some((wav, _, _)) = cpal_result {
                    let _ = std::fs::remove_file(wav);
                }
                return Ok(winrt_said);
            }

            // WinRT returned empty (0 guesses/phrases or unavailable): fall back to local Whisper!
            let Some((wav, samples, peak)) = cpal_result else {
                return Err("I couldn't hear anything. Check Settings → Privacy & security → Microphone and Settings → System → Sound.".into());
            };
            let res = if samples < 1_600 || peak < 0.002 {
                Err(format!(
                    "Your microphone was silent (peak {:.3}). Check that it isn't muted and is the default input in Settings → System → Sound.",
                    peak
                ))
            } else {
                crate::ask::diag("voice fallback: running local whisper tiny.en");
                crate::whisper_local::transcribe_wav(&wav, on_status)
            };
            let _ = std::fs::remove_file(&wav);
            res
        }),
    })
}

#[cfg(target_os = "macos")]
pub fn start(_live: impl Fn(String) + Send + Sync + 'static) -> Result<Listening, String> {
    let mic = start_cpal_capture()?;
    Ok(Listening {
        stop: Box::new(move |on_status| {
            let wav = mic.wav_path;
            let (samples, peak) = (mic.stop)();
            let res = if samples < 1_600 || peak < 0.002 {
                Err("Your microphone was silent. Check System Settings → Sound → Input.".into())
            } else {
                crate::whisper_local::transcribe_wav(&wav, on_status)
            };
            let _ = std::fs::remove_file(&wav);
            res
        }),
    })
}

#[cfg(target_os = "linux")]
pub fn start(_live: impl Fn(String) + Send + Sync + 'static) -> Result<Listening, String> {
    Err("Hold-to-talk is on Windows and macOS for now. Tap the hotkey and type instead.".into())
}
