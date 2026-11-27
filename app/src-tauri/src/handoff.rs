//! Opening the next tool: its desktop app, its CLI in a terminal, or its web
//! app. The prompt always goes to the clipboard first, so whatever opens, the
//! user only has to paste.

use aiuc_core::{handoff, model::Tool};
use serde::{Deserialize, Serialize};
use std::{io::Write, path::{Path, PathBuf}, process::{Command, Stdio}};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Dest {
    App,
    Terminal,
    Web,
}

#[derive(Debug, Clone, Serialize)]
pub struct DestOption {
    pub dest: Dest,
    pub label: String,
}

pub fn web_url(target: Tool) -> &'static str {
    match target {
        Tool::ClaudeCode => "https://claude.ai/code",
        Tool::Codex => "https://chatgpt.com/codex",
        Tool::Gemini => "https://gemini.google.com/app",
    }
}

/// How to start a desktop app.
#[derive(Debug, Clone)]
enum AppLaunch {
    #[cfg_attr(target_os = "linux", allow(dead_code))]
    Path(PathBuf),
    #[cfg_attr(not(windows), allow(dead_code))]
    Url(&'static str),
}

/// The vendor's desktop app, if installed. Claude Code lives in the Claude
/// app's Code tab; Codex has its own app; Gemini has none for coding.
fn find_app(target: Tool) -> Option<AppLaunch> {
    let name = match target {
        Tool::ClaudeCode => "Claude",
        Tool::Codex => "Codex",
        Tool::Gemini => return None,
    };
    #[cfg(target_os = "macos")]
    {
        let home = dirs_home();
        let mut candidates = vec![PathBuf::from(format!("/Applications/{name}.app"))];
        if let Some(h) = home { candidates.push(h.join(format!("Applications/{name}.app"))); }
        candidates.into_iter().find(|p| p.is_dir()).map(AppLaunch::Path)
    }
    #[cfg(windows)]
    {
        let local = std::env::var_os("LOCALAPPDATA").map(PathBuf::from);
        let exes: Vec<PathBuf> = match target {
            Tool::ClaudeCode => local.iter().flat_map(|l| [l.join(r"AnthropicClaude\claude.exe"), l.join(r"Programs\Claude\Claude.exe")]).collect(),
            _ => local.iter().flat_map(|l| [l.join(r"Programs\Codex\Codex.exe"), l.join(r"Codex\Codex.exe")]).collect(),
        };
        if let Some(p) = exes.into_iter().find(|p| p.is_file()) { return Some(AppLaunch::Path(p)); }
        // Store (MSIX) installs have no exe we can see, but register a link scheme.
        let scheme = if target == Tool::ClaudeCode { "claude" } else { "codex" };
        url_scheme_registered(scheme).then_some(AppLaunch::Url(if target == Tool::ClaudeCode { "claude://" } else { "codex://" }))
    }
    #[cfg(target_os = "linux")]
    {
        let _ = name;
        None
    }
}

#[cfg(target_os = "macos")]
fn dirs_home() -> Option<PathBuf> { std::env::var_os("HOME").map(PathBuf::from) }

#[cfg(windows)]
fn url_scheme_registered(scheme: &str) -> bool {
    use std::os::windows::process::CommandExt;
    Command::new("reg").creation_flags(0x08000000)
        .args(["query", &format!(r"HKCR\{scheme}"), "/v", "URL Protocol"])
        .stdout(Stdio::null()).stderr(Stdio::null())
        .status().is_ok_and(|s| s.success())
}

fn app_label(target: Tool) -> &'static str {
    match target { Tool::ClaudeCode => "Claude app", Tool::Codex => "Codex app", Tool::Gemini => "Gemini" }
}

/// Where `target` can open, best first: the desktop app, then the CLI in a
/// terminal, then the web. Claude Code and Codex prefer their app; Gemini
/// opens its CLI. The web is always there.
pub fn destinations(target: Tool, local_folder: bool) -> Vec<DestOption> {
    let mut out = Vec::new();
    if find_app(target).is_some() { out.push(DestOption { dest: Dest::App, label: app_label(target).into() }); }
    if local_folder && handoff::cli(target).is_some() { out.push(DestOption { dest: Dest::Terminal, label: "Terminal".into() }); }
    out.push(DestOption { dest: Dest::Web, label: "Web".into() });
    out
}

fn copy(mut command: Command, prompt: &str) -> bool {
    let Ok(mut child) = command.stdin(Stdio::piped()).stdout(Stdio::null()).stderr(Stdio::null()).spawn() else { return false };
    let written = child.stdin.take().unwrap().write_all(prompt.as_bytes()).is_ok();
    child.wait().is_ok_and(|s| s.success()) && written
}

#[cfg(windows)]
fn powershell() -> Result<PathBuf, String> {
    Ok(PathBuf::from(std::env::var_os("SystemRoot").ok_or("Windows system folder unavailable.")?)
        .join(r"System32\WindowsPowerShell\v1.0\powershell.exe"))
}

pub fn copy_to_clipboard(prompt: &str) -> Result<(), String> {
    #[cfg(windows)]
    let ok = {
        use std::os::windows::process::CommandExt;
        let mut clip = Command::new(powershell()?);
        clip.creation_flags(0x08000000).args(["-NoProfile", "-Command",
            "[Console]::InputEncoding=[Text.UTF8Encoding]::new($false); Set-Clipboard -Value ([Console]::In.ReadToEnd()) -ErrorAction Stop"]);
        copy(clip, prompt)
    };
    #[cfg(target_os = "macos")]
    let ok = copy(Command::new("/usr/bin/pbcopy"), prompt);
    #[cfg(target_os = "linux")]
    let ok = {
        let mut clip = Command::new("xclip");
        clip.args(["-selection", "clipboard"]);
        copy(Command::new("wl-copy"), prompt) || copy(clip, prompt)
    };
    if ok { Ok(()) } else { Err("Couldn't copy to the clipboard.".into()) }
}

fn open_app(target: Tool) -> bool {
    match find_app(target) {
        #[cfg(target_os = "macos")]
        Some(AppLaunch::Path(p)) => Command::new("/usr/bin/open").arg("-a").arg(p).status().is_ok_and(|s| s.success()),
        #[cfg(not(target_os = "macos"))]
        Some(AppLaunch::Path(p)) => Command::new(p).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn().is_ok(),
        Some(AppLaunch::Url(u)) => tauri_plugin_opener::open_url(u, None::<&str>).is_ok(),
        None => false,
    }
}

/// Start `target`'s CLI in a new terminal in `cwd`, with `first` as its first message.
pub fn open_terminal(cwd: &Path, target: Tool, first: &str) -> Result<bool, String> {
    let cli = handoff::cli(target).ok_or("That CLI is no longer installed.")?;
    #[cfg(windows)]
    let started = {
        use std::os::windows::process::CommandExt;
        let ps = powershell()?;
        // The whole command travels as -EncodedCommand (base64 UTF-16LE), with
        // the folder and CLI path as single-quoted literals. Nothing depends on
        // environment variables (Windows Terminal starts tabs with its own
        // environment) and no path can break out of its quotes.
        let script = powershell_script(cwd, &cli, target == Tool::Gemini, first);
        let encoded = encode_powershell(&script);
        let args = ["-NoProfile", "-NoExit", "-EncodedCommand", encoded.as_str()];
        let terminal = handoff::executable("wt").is_some_and(|wt| Command::new(wt)
            .args(["-w", "new", "-d"]).arg(cwd).arg(&ps).args(args)
            .spawn().is_ok());
        terminal || Command::new(ps)
            .creation_flags(0x00000010).current_dir(cwd).args(args).spawn().is_ok()
    };
    #[cfg(target_os = "macos")]
    let started = {
        let script = r#"on run argv
set cmd to "cd -- " & quoted form of (item 1 of argv) & " && " & quoted form of (item 2 of argv)
repeat with i from 3 to count of argv
set cmd to cmd & " " & quoted form of (item i of argv)
end repeat
tell application "Terminal"
activate
do script cmd
end tell
end run"#;
        let mut cmd = Command::new("/usr/bin/osascript");
        cmd.args(["-e", script, "--"]).arg(cwd).arg(cli);
        if target == Tool::Gemini { cmd.arg("-i"); }
        cmd.arg(first).status().is_ok_and(|s| s.success())
    };
    #[cfg(target_os = "linux")]
    let started = ["x-terminal-emulator", "xterm"].iter().any(|terminal| {
        let mut cmd = Command::new(terminal);
        cmd.current_dir(cwd).arg("-e").arg(&cli);
        if target == Tool::Gemini { cmd.arg("-i"); }
        cmd.arg(first).spawn().is_ok()
    });
    Ok(started)
}

/// Hand the prompt to `target`. `cwd` is the project folder on this computer
/// (absent for a repo that's only in the cloud). Returns a short line for the
/// user saying what happened.
pub fn launch(cwd: Option<&Path>, target: Tool, dest: Dest, prompt: &str) -> Result<String, String> {
    let name = crate::tool_name(target);
    // Local tools read the file; web and app sessions get the pasted prompt.
    if let Some(dir) = cwd {
        handoff::save(dir, prompt).map_err(|e| format!("Couldn't save the handoff: {e}"))?;
    }
    copy_to_clipboard(prompt)?;
    match dest {
        Dest::Terminal => {
            let dir = cwd.ok_or("Open this repo on your computer to use the terminal.")?;
            if !open_terminal(dir, target, &format!("Read {} and continue", handoff::FILENAME))? {
                return Err("Handoff copied, but no terminal opened.".into());
            }
            Ok(format!("{name} is starting in a terminal."))
        }
        Dest::App => {
            if !open_app(target) {
                tauri_plugin_opener::open_url(web_url(target), None::<&str>).map_err(|e| e.to_string())?;
                return Ok(format!("The {} didn't open, so the web app did. Paste the prompt there.", app_label(target)));
            }
            Ok(format!("Prompt copied. Paste it into {name} in the {}.", app_label(target)))
        }
        Dest::Web => {
            tauri_plugin_opener::open_url(web_url(target), None::<&str>).map_err(|e| e.to_string())?;
            Ok(format!("Prompt copied. Pick the repo in {name} on the web and paste it."))
        }
    }
}

#[cfg_attr(not(windows), allow(dead_code))]
/// A PowerShell literal: single quotes, with embedded quotes doubled.
fn ps_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "''"))
}

#[cfg_attr(not(windows), allow(dead_code))]
/// What the new terminal runs: go to the project and start the CLI with the
/// first message already typed in.
pub fn powershell_script(cwd: &Path, cli: &Path, interactive_flag: bool, first: &str) -> String {
    format!(
        "Set-Location -LiteralPath {}; & {}{} {}",
        ps_quote(&cwd.to_string_lossy()),
        ps_quote(&cli.to_string_lossy()),
        if interactive_flag { " -i" } else { "" },
        ps_quote(first)
    )
}

#[cfg_attr(not(windows), allow(dead_code))]
/// PowerShell's -EncodedCommand format: base64 of the UTF-16LE script.
pub fn encode_powershell(script: &str) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let bytes: Vec<u8> = script.encode_utf16().flat_map(|u| u.to_le_bytes()).collect();
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b = [chunk[0], *chunk.get(1).unwrap_or(&0), *chunk.get(2).unwrap_or(&0)];
        let n = (b[0] as u32) << 16 | (b[1] as u32) << 8 | b[2] as u32;
        out.push(T[(n >> 18) as usize & 63] as char);
        out.push(T[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 { T[(n >> 6) as usize & 63] as char } else { '=' });
        out.push(if chunk.len() > 2 { T[n as usize & 63] as char } else { '=' });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn powershell_script_quotes_paths() {
        let first = "Read .aiuc-handoff.md and continue";
        let s = powershell_script(Path::new(r"C:\Users\o'neil\my proj"), Path::new(r"C:\npm\codex.cmd"), false, first);
        assert_eq!(s, r"Set-Location -LiteralPath 'C:\Users\o''neil\my proj'; & 'C:\npm\codex.cmd' 'Read .aiuc-handoff.md and continue'");
        assert!(powershell_script(Path::new("/p"), Path::new("/g"), true, first).contains("& '/g' -i 'Read"));
    }

    #[test]
    fn encodes_utf16le_base64() {
        // [Convert]::ToBase64String([Text.Encoding]::Unicode.GetBytes("dir"))
        assert_eq!(encode_powershell("dir"), "ZABpAHIA");
        assert_eq!(encode_powershell("ab"), "YQBiAA==");
    }

    #[test]
    fn web_is_always_a_destination() {
        for t in [Tool::ClaudeCode, Tool::Codex, Tool::Gemini] {
            assert_eq!(destinations(t, false).last().unwrap().dest, Dest::Web);
            assert!(!destinations(t, false).iter().any(|d| d.dest == Dest::Terminal), "no terminal without a local folder");
        }
    }
}
