//! "Verified done": when an agent finishes, run the project's tests in its
//! folder and record whether they passed. Only the result and how long it took
//! are stored; the output is kept in memory for the widget to show.

use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// How the tests are run in `dir`. A first line in `.aiuc/verify` wins;
/// otherwise it's guessed from the project's files.
pub fn detect(dir: &Path) -> Option<String> {
    if let Ok(s) = std::fs::read_to_string(dir.join(".aiuc").join("verify")) {
        if let Some(l) = s.lines().map(str::trim).find(|l| !l.is_empty() && !l.starts_with('#')) {
            return Some(l.to_owned());
        }
    }
    if dir.join("Cargo.toml").is_file() { return Some("cargo test --quiet".into()); }
    if let Ok(pkg) = std::fs::read_to_string(dir.join("package.json")) {
        let test = serde_json::from_str::<serde_json::Value>(&pkg).ok()
            .and_then(|v| v.pointer("/scripts/test").and_then(|t| t.as_str()).map(str::to_owned));
        if test.is_some_and(|t| !t.contains("no test specified")) {
            let pm = if dir.join("pnpm-lock.yaml").is_file() { "pnpm" } else if dir.join("yarn.lock").is_file() { "yarn" } else if dir.join("bun.lockb").is_file() || dir.join("bun.lock").is_file() { "bun" } else { "npm" };
            return Some(format!("{pm} test"));
        }
    }
    if dir.join("go.mod").is_file() { return Some("go test ./...".into()); }
    let py = ["pytest.ini", "pyproject.toml", "setup.cfg", "tox.ini"].iter().any(|f| dir.join(f).is_file());
    if py && (dir.join("tests").is_dir() || dir.join("test").is_dir()) { return Some("python -m pytest -q".into()); }
    if dir.join("Makefile").is_file() && std::fs::read_to_string(dir.join("Makefile")).is_ok_and(|m| m.lines().any(|l| l.starts_with("test:"))) {
        return Some("make test".into());
    }
    None
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    Passed,
    Failed,
    TimedOut,
    /// The command couldn't start.
    Error(String),
}

impl Outcome {
    pub fn as_str(&self) -> &'static str {
        match self { Outcome::Passed => "passed", Outcome::Failed => "failed", Outcome::TimedOut => "timed_out", Outcome::Error(_) => "error" }
    }
}

pub struct Run {
    pub outcome: Outcome,
    pub secs: f64,
    /// The last lines of output.
    pub tail: String,
}

fn shell(cmd: &str) -> Command {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let mut c = Command::new("cmd");
        c.arg("/C").raw_arg(cmd);
        c.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
        c
    }
    #[cfg(not(windows))]
    {
        let mut c = Command::new("sh");
        c.args(["-c", cmd]);
        c
    }
}

fn tail(s: &str, lines: usize) -> String {
    let all: Vec<&str> = s.lines().collect();
    all[all.len().saturating_sub(lines)..].join("\n")
}

/// Run `cmd` in `dir`, stopping it after `timeout`.
pub fn run(dir: &Path, cmd: &str, timeout: Duration) -> Run {
    let start = Instant::now();
    let child = shell(cmd).current_dir(dir).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped())
        .env("CI", "1").env("NO_COLOR", "1").spawn();
    let mut child = match child {
        Ok(c) => c,
        Err(e) => return Run { outcome: Outcome::Error(e.to_string()), secs: 0.0, tail: String::new() },
    };
    // Read both pipes on threads so a chatty test run can't block on a full pipe.
    let mut readers = Vec::new();
    for pipe in [child.stdout.take().map(|p| Box::new(p) as Box<dyn Read + Send>), child.stderr.take().map(|p| Box::new(p) as Box<dyn Read + Send>)].into_iter().flatten() {
        readers.push(std::thread::spawn(move || {
            let mut pipe = pipe;
            let mut buf = Vec::new();
            let _ = pipe.read_to_end(&mut buf);
            // Keep the end only.
            let keep = buf.len().saturating_sub(64 * 1024);
            String::from_utf8_lossy(&buf[keep..]).into_owned()
        }));
    }
    let outcome = loop {
        match child.try_wait() {
            Ok(Some(status)) => break if status.success() { Outcome::Passed } else { Outcome::Failed },
            Ok(None) if start.elapsed() > timeout => {
                // The shell's children too (cargo, node…).
                #[cfg(windows)]
                { use std::os::windows::process::CommandExt; let _ = Command::new("taskkill").args(["/T", "/F", "/PID", &child.id().to_string()]).creation_flags(0x0800_0000).output(); }
                let _ = child.kill();
                let _ = child.wait();
                break Outcome::TimedOut;
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(200)),
            Err(e) => break Outcome::Error(e.to_string()),
        }
    };
    let out: Vec<String> = readers.into_iter().filter_map(|r| r.join().ok()).collect();
    Run { outcome, secs: start.elapsed().as_secs_f64(), tail: tail(&out.join("\n"), 40) }
}
