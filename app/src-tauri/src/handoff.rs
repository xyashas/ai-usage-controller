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
        let invoke = if target == Tool::Gemini { "& $env:AIUC_CLI -i $env:AIUC_INITIAL" } else { "& $env:AIUC_CLI $env:AIUC_INITIAL" };
        let args = ["-NoProfile", "-NoExit", "-Command", invoke];
        let terminal = handoff::executable("wt").is_some_and(|wt| Command::new(wt)
            .env("AIUC_CLI", &cli).env("AIUC_INITIAL", INITIAL).current_dir(cwd)
            .args(["-w", "new", "new-tab", "--inheritEnvironment", "-d", "."]).arg(&ps).args(args)
            .status().is_ok_and(|s| s.success()));
        terminal || Command::new(ps).env("AIUC_CLI", cli).env("AIUC_INITIAL", INITIAL)
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
