//! Hold-to-talk: listen while the Ask hotkey is held, using the system's own
//! speech recognition (Windows for now). Audio never leaves the OS recognizer
//! and nothing is recorded by this app.

#[cfg(windows)]
use std::sync::{Arc, Mutex, Once};

pub struct Listening {
    stop: Box<dyn FnOnce() -> Result<String, String> + Send>,
}

impl Listening {
    /// Stop listening and return what was said.
    pub fn finish(self) -> Result<String, String> {
        (self.stop)()
    }
}

/// Prime Windows microphone permission & audio capture endpoint once per process
/// so WinRT SpeechRecognizer doesn't silently receive 0 audio frames.
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

/// Start listening. `live` gets the words so far as you speak (for captions).
#[cfg(windows)]
pub fn start(live: impl Fn(String) + Send + Sync + 'static) -> Result<Listening, String> {
    use windows::Foundation::{TimeSpan, TypedEventHandler};
    use windows::Media::SpeechRecognition::{
        SpeechContinuousRecognitionCompletedEventArgs,
        SpeechContinuousRecognitionResultGeneratedEventArgs,
        SpeechContinuousRecognitionSession,
        SpeechRecognitionHypothesisGeneratedEventArgs,
        SpeechRecognitionResultStatus,
        SpeechRecognizer,
    };
    let live = Arc::new(live);
    // WinRT objects live on one thread, which owns the session until told to stop.
    let text = Arc::new(Mutex::new(Vec::<String>::new()));
    // The newest guess (what the captions show). Windows only commits a phrase
    // after a pause, so if you let go mid-sentence the guess is all there is.
    let guess = Arc::new(Mutex::new(String::new()));
    let counts = Arc::new(Mutex::new((0u32, 0u32))); // (guesses, final phrases), for diagnostics
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
            // Prefer explicit system speech language, falling back to default constructor.
            let rec = match SpeechRecognizer::SystemSpeechLanguage() {
                Ok(lang) => {
                    let tag = lang.LanguageTag().map(|s| s.to_string()).unwrap_or_else(|_| "unknown".into());
                    match SpeechRecognizer::Create(&lang) {
                        Ok(r) => {
                            crate::ask::diag(&format!("voice lang: {tag} (SystemSpeechLanguage)"));
                            r
                        }
                        Err(e) => {
                            crate::ask::diag(&format!(
                                "voice lang: Create({tag}) err=0x{:08X}, falling back to default",
                                e.code().0 as u32
                            ));
                            SpeechRecognizer::new()?
                        }
                    }
                }
                Err(_) => SpeechRecognizer::new()?,
            };
            // Generous initial silence (10 s) so waiting after the chime doesn't abort,
            // and 1.2 s end silence so phrases commit cleanly without cutting off mid-thought.
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
            // Words as you say them: everything final so far + the current guess.
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
                    "voice init err=0x{:08X} ({})",
                    e.code().0 as u32,
                    e.message()
                ));
                let _ = ready_tx.send(Err(explain(e.code().0 as u32, &e.message())));
                return;
            }
        };
        let _ = stop_rx.recv();
        // Stopping flushes the last phrase as a final result.
        if let Ok(s) = rec.ContinuousRecognitionSession() { let _ = s.StopAsync().and_then(|a| a.get()); }
        let _ = done_tx.send(());
    });
    ready_rx.recv().map_err(|_| "Speech recognition didn't start.".to_string())??;
    Ok(Listening {
        stop: Box::new(move || {
            let _ = stop_tx.send(());
            let _ = done_rx.recv_timeout(std::time::Duration::from_secs(4));
            let finals = text.lock().map(|v| v.join(" ")).unwrap_or_default();
            let last_guess = guess.lock().map(|g| g.clone()).unwrap_or_default();
            // Prefer what Windows committed; fall back to the captions if it
            // committed less (you let go before it caught up).
            let said = if last_guess.trim().len() > finals.trim().len() { last_guess } else { finals };
            let (g, f) = counts.lock().map(|c| *c).unwrap_or((0, 0));
            crate::ask::diag(&format!("voice heard: {g} guesses, {f} phrases, {} words", said.split_whitespace().count()));
            if said.trim().is_empty() && g == 0 && f == 0 {
                return Err("I couldn't hear anything. Check Windows Settings → Privacy & security → Microphone: \"Let desktop apps access your microphone\" is on, and your mic is the default input (Settings → System → Sound).".into());
            }
            Ok(said.trim().to_owned())
        }),
    })
}

#[cfg(windows)]
fn explain(code: u32, msg: &str) -> String {
    match code {
        // SPERR_PRIVACY / speech privacy policy not accepted
        0x80045509 => "Turn on Windows speech: Settings → Privacy & security → Speech → Online speech recognition. Then hold the hotkey again.".into(),
        // No microphone
        0x8004503A | 0x80070490 => "No microphone found. Plug one in or check Settings → System → Sound.".into(),
        // Access denied (microphone privacy setting off)
        0x80070005 => "Microphone access is blocked. Turn on Settings → Privacy & security → Microphone → \"Let desktop apps access your microphone\".".into(),
        _ => format!("Speech recognition isn't available ({msg})."),
    }
}

#[cfg(not(windows))]
pub fn start(_live: impl Fn(String) + Send + Sync + 'static) -> Result<Listening, String> {
    Err("Hold-to-talk is on Windows for now. Tap the hotkey and use the mic button instead.".into())
}
