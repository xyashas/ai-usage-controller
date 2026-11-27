//! Clipboard watcher for "Speaks up first" error suggestions.
//!
//! Polls clipboard text every 1.5 s ONLY while `settings.suggest_chips` is on.
//! Privacy rule: never store or log clipboard text — keep only a hash in memory
//! to detect changes.

use aiuc_core::suggest::{self, Kind, Suggestion};
use aiuc_core::Engine;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tauri::{AppHandle, Emitter};

const POLL_INTERVAL: Duration = Duration::from_millis(1500);

fn hash_text(text: &str) -> u64 {
    let mut h = DefaultHasher::new();
    text.hash(&mut h);
    h.finish()
}

fn read_clipboard() -> Option<String> {
    #[cfg(target_os = "macos")]
    let out = Command::new("/usr/bin/pbpaste")
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;

    #[cfg(windows)]
    let out = {
        use std::os::windows::process::CommandExt;
        let ps = std::env::var_os("SystemRoot")
            .map(std::path::PathBuf::from)?
            .join(r"System32\WindowsPowerShell\v1.0\powershell.exe");
        Command::new(ps)
            .creation_flags(0x08000000)
            .args([
                "-NoProfile",
                "-Command",
                "[Console]::OutputEncoding=[Text.UTF8Encoding]::new($false); Get-Clipboard -Raw -ErrorAction SilentlyContinue",
            ])
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .output()
            .ok()?
    };

    #[cfg(target_os = "linux")]
    let out = Command::new("wl-paste")
        .arg("--no-newline")
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()
        .filter(|o| o.status.success())
        .or_else(|| {
            Command::new("xclip")
                .args(["-o", "-selection", "clipboard"])
                .stdin(Stdio::null())
                .stderr(Stdio::null())
                .output()
                .ok()
        })?;

    if !out.status.success() {
        return None;
    }
    let s = String::from_utf8_lossy(&out.stdout).into_owned();
    (!s.trim().is_empty()).then_some(s)
}

pub fn spawn(app: AppHandle, engine: Arc<Mutex<Engine>>) {
    std::thread::spawn(move || {
        let mut last_hash: Option<u64> = None;
        let mut seeded = false;

        loop {
            let enabled = engine
                .lock()
                .ok()
                .map(|e| e.settings.suggest_chips)
                .unwrap_or(false);

            if !enabled {
                seeded = false;
                last_hash = None;
                std::thread::sleep(POLL_INTERVAL);
                continue;
            }

            if let Some(text) = read_clipboard() {
                let h = hash_text(&text);
                if !seeded {
                    // Seed with existing clipboard on enable/startup so we don't fire on stale content.
                    last_hash = Some(h);
                    seeded = true;
                } else if last_hash != Some(h) {
                    last_hash = Some(h);
                    if suggest::looks_like_error(&text) {
                        let suggestion = Suggestion {
                            id: format!("clip:{h:x}"),
                            kind: Kind::CopiedError,
                            title: "Fix this error?".into(),
                            detail: "Copied error detected".into(),
                            action: "ask_fix".into(),
                            text: None,
                        };
                        let _ = app.emit("suggestion", &suggestion);
                    }
                }
            } else if !seeded {
                seeded = true;
            }

            std::thread::sleep(POLL_INTERVAL);
        }
    });
}
