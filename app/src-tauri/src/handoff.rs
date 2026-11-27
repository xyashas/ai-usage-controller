use aiuc_core::{handoff, model::Tool};
use std::{io::Write, path::Path, process::{Command, Stdio}};

const INITIAL: &str = "Read .aiuc-handoff.md and continue";

fn copy(mut command: Command, prompt: &str) -> bool {
    let Ok(mut child) = command.stdin(Stdio::piped()).stdout(Stdio::null()).stderr(Stdio::null()).spawn() else { return false };
    let written = child.stdin.take().unwrap().write_all(prompt.as_bytes()).is_ok();
    child.wait().is_ok_and(|s| s.success()) && written
}

pub fn launch(cwd: &Path, target: Tool, prompt: &str) -> Result<(), String> {
    let cli = handoff::cli(target).ok_or("The destination CLI is no longer on PATH.")?;
    handoff::save(cwd, prompt).map_err(|e| format!("Could not save the handoff: {e}"))?;
    let copy_error = "Handoff saved. Clipboard unavailable; retry to launch.";
    #[cfg(windows)]
    let started = {
        use std::os::windows::process::CommandExt;
        let ps = std::path::PathBuf::from(std::env::var_os("SystemRoot").ok_or("Windows system folder unavailable.")?)
            .join(r"System32\WindowsPowerShell\v1.0\powershell.exe");
        let mut clip = Command::new(&ps);
        clip.creation_flags(0x08000000).args(["-NoProfile", "-Command",
            "[Console]::InputEncoding=[Text.UTF8Encoding]::new($false); Set-Clipboard -Value ([Console]::In.ReadToEnd()) -ErrorAction Stop"]);
        if !copy(clip, prompt) { return Err(copy_error.into()); }
        // The whole command travels as -EncodedCommand (base64 UTF-16LE), with
        // the folder and CLI path as single-quoted literals. Nothing depends on
        // environment variables (Windows Terminal starts tabs with its own
        // environment) and no path can break out of its quotes.
        let script = powershell_script(cwd, &cli, target == Tool::Gemini);
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
        if !copy(Command::new("/usr/bin/pbcopy"), prompt) { return Err(copy_error.into()); }
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
        cmd.arg(INITIAL).status().is_ok_and(|s| s.success())
    };
    #[cfg(target_os = "linux")]
    let started = {
        let mut clip = Command::new("xclip");
        clip.args(["-selection", "clipboard"]);
        if !copy(Command::new("wl-copy"), prompt) && !copy(clip, prompt) { return Err(copy_error.into()); }
        ["x-terminal-emulator", "xterm"].iter().any(|terminal| {
            let mut cmd = Command::new(terminal);
            cmd.current_dir(cwd).arg("-e").arg(&cli);
            if target == Tool::Gemini { cmd.arg("-i"); }
            cmd.arg(INITIAL).spawn().is_ok()
        })
    };
    if !started { return Err("Handoff saved and copied. Could not open a terminal.".into()); }
    Ok(())
}

#[cfg_attr(not(windows), allow(dead_code))]
/// A PowerShell literal: single quotes, with embedded quotes doubled.
fn ps_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "''"))
}

#[cfg_attr(not(windows), allow(dead_code))]
/// What the new terminal runs: go to the project and start the CLI with the
/// first message already typed in.
pub fn powershell_script(cwd: &Path, cli: &Path, interactive_flag: bool) -> String {
    format!(
        "Set-Location -LiteralPath {}; & {}{} {}",
        ps_quote(&cwd.to_string_lossy()),
        ps_quote(&cli.to_string_lossy()),
        if interactive_flag { " -i" } else { "" },
        ps_quote(INITIAL)
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
        let s = powershell_script(Path::new(r"C:\Users\o'neil\my proj"), Path::new(r"C:\npm\codex.cmd"), false);
        assert_eq!(s, r"Set-Location -LiteralPath 'C:\Users\o''neil\my proj'; & 'C:\npm\codex.cmd' 'Read .aiuc-handoff.md and continue'");
        assert!(powershell_script(Path::new("/p"), Path::new("/g"), true).contains("& '/g' -i 'Read"));
    }

    #[test]
    fn encodes_utf16le_base64() {
        // [Convert]::ToBase64String([Text.Encoding]::Unicode.GetBytes("dir"))
        assert_eq!(encode_powershell("dir"), "ZABpAHIA");
        assert_eq!(encode_powershell("ab"), "YQBiAA==");
    }
}
