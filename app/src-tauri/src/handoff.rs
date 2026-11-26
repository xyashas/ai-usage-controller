use std::{io::Write, path::Path, process::{Command, Stdio}};

fn copy(mut command: Command, prompt: &str) -> bool {
    let Ok(mut child) = command.stdin(Stdio::piped()).stdout(Stdio::null()).stderr(Stdio::null()).spawn() else { return false };
    let written = child.stdin.take().unwrap().write_all(prompt.as_bytes()).is_ok();
    child.wait().is_ok_and(|s| s.success()) && written
}

pub fn open(cwd: &Path, prompt: &str) -> Result<(), String> {
    #[cfg(windows)]
    let (copied, started) = {
        use std::os::windows::process::CommandExt;
        let ps = std::path::PathBuf::from(std::env::var_os("SystemRoot").ok_or("Windows system directory unavailable.")?)
            .join(r"System32\WindowsPowerShell\v1.0\powershell.exe");
        let mut clip = Command::new(&ps);
        clip.creation_flags(0x08000000).args(["-NoProfile", "-Command",
            "[Console]::InputEncoding=[Text.UTF8Encoding]::new($false); Set-Clipboard -Value ([Console]::In.ReadToEnd()) -ErrorAction Stop"]);
        let copied = copy(clip, prompt);
        let started = copied && Command::new(ps).creation_flags(0x00000010).current_dir(cwd)
            .args(["-NoProfile", "-NoExit", "-Command", "if (Get-Command codex.cmd -ErrorAction SilentlyContinue) { codex.cmd } else { codex }"]).spawn().is_ok();
        (copied, started)
    };
    #[cfg(target_os = "macos")]
    let (copied, started) = {
        let copied = copy(Command::new("/usr/bin/pbcopy"), prompt);
        let script = r#"on run argv
tell application "Terminal"
activate
do script "cd -- " & quoted form of (item 1 of argv) & " && codex"
end tell
end run"#;
        let started = copied && Command::new("/usr/bin/osascript").args(["-e", script, "--"]).arg(cwd)
            .status().is_ok_and(|s| s.success());
        (copied, started)
    };
    #[cfg(target_os = "linux")]
    let (copied, started) = {
        let mut xclip = Command::new("xclip");
        xclip.args(["-selection", "clipboard"]);
        let copied = copy(Command::new("wl-copy"), prompt) || copy(xclip, prompt);
        let started = copied && ["x-terminal-emulator", "xterm"].iter().any(|term|
            Command::new(term).current_dir(cwd).args(["-e", "codex"]).spawn().is_ok());
        (copied, started)
    };
    if !copied { return Err("Could not copy the handoff to the clipboard.".into()); }
    if !started { return Err("Handoff copied. Open Codex in the project folder and paste it.".into()); }
    Ok(())
}
