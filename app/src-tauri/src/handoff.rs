use std::{io::Write, path::Path, process::{Command, Stdio}};

fn copy(mut command: Command, prompt: &str) -> bool {
    let Ok(mut child) = command.stdin(Stdio::piped()).stdout(Stdio::null()).stderr(Stdio::null()).spawn() else { return false };
    let written = child.stdin.take().unwrap().write_all(prompt.as_bytes()).is_ok();
    child.wait().is_ok_and(|s| s.success()) && written
}

pub fn open(cwd: &Path, prompt: &str) -> Result<(), String> {
    let codex = aiuc_core::handoff::codex_path().ok_or("Install Codex to hand off")?;
    aiuc_core::handoff::save(cwd, prompt).map_err(|_| "Could not write .aiuc-handoff.md in the project folder.")?;
    const INITIAL: &str = "Read .aiuc-handoff.md and continue";
    #[cfg(windows)]
    let started = {
        use std::os::windows::process::CommandExt;
        let ps = std::path::PathBuf::from(std::env::var_os("SystemRoot").ok_or("Windows system directory unavailable.")?)
            .join(r"System32\WindowsPowerShell\v1.0\powershell.exe");
        let mut clip = Command::new(&ps);
        clip.creation_flags(0x08000000).args(["-NoProfile", "-Command",
            "[Console]::InputEncoding=[Text.UTF8Encoding]::new($false); Set-Clipboard -Value ([Console]::In.ReadToEnd()) -ErrorAction Stop"]);
        let _ = copy(clip, prompt);
        Command::new(ps).creation_flags(0x00000010).current_dir(cwd).env("AIUC_CODEX", codex).env("AIUC_INITIAL", INITIAL)
            .args(["-NoProfile", "-NoExit", "-Command", "& $env:AIUC_CODEX $env:AIUC_INITIAL"]).spawn().is_ok()
    };
    #[cfg(target_os = "macos")]
    let started = {
        let _ = copy(Command::new("/usr/bin/pbcopy"), prompt);
        let script = r#"on run argv
tell application "Terminal"
activate
do script "cd -- " & quoted form of (item 1 of argv) & " && " & quoted form of (item 2 of argv) & " " & quoted form of (item 3 of argv)
end tell
end run"#;
        Command::new("/usr/bin/osascript").args(["-e", script, "--"]).arg(cwd).arg(codex).arg(INITIAL)
            .status().is_ok_and(|s| s.success())
    };
    #[cfg(target_os = "linux")]
    let started = {
        let mut xclip = Command::new("xclip");
        xclip.args(["-selection", "clipboard"]);
        let _ = copy(Command::new("wl-copy"), prompt) || copy(xclip, prompt);
        ["x-terminal-emulator", "xterm"].iter().any(|term|
            Command::new(term).current_dir(cwd).arg("-e").arg(&codex).arg(INITIAL).spawn().is_ok())
    };
    if !started { return Err("Handoff saved. Open Codex in the project folder and ask it to read .aiuc-handoff.md.".into()); }
    Ok(())
}
