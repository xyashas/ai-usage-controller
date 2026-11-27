// Prevents an extra console window on Windows in release.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // Claude Code and Codex call this same program as their hook.
    if aiuc_app_lib::hook_main() {
        return;
    }
    aiuc_app_lib::run()
}
