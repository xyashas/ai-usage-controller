//! Regression tests for Voice & Ask Windows fixes:
//! - Auto prefers Claude Code (haiku stream-json), falls back to Codex, never Gemini
//! - StreamReader emits text from Claude "result" event when no partial/assistant text was emitted
//! - Clipboard-only Fix / Explain / FindBug prompts include <copied_text>
//! - Post-answer voice commands: paste that, copy that, shorter, send it to Codex, remember that…, save this

use aiuc_core::ask::{
    build_prompt, command, pick, voice_intent, Action, Context, Intent, Mode, Room, StreamReader,
};
use aiuc_core::model::Tool;
use std::path::Path;

#[test]
fn auto_uses_claude_then_codex_never_gemini() {
    let all = vec![
        Room { tool: Tool::ClaudeCode, installed: true, used_pct: Some(50.0) },
        Room { tool: Tool::Codex, installed: true, used_pct: Some(10.0) },
        Room { tool: Tool::Gemini, installed: true, used_pct: Some(0.0) },
    ];
    let p = pick(&all, None).unwrap();
    assert_eq!(p.tool, Tool::ClaudeCode, "Auto must prefer Claude Code even when Codex/Gemini have lower usage");

    let cmd = command(Tool::ClaudeCode, Path::new("claude.cmd"), Mode::Quick, "hello", None);
    assert_eq!(cmd.program, Path::new("claude.cmd"));
    assert!(cmd.args.windows(2).any(|w| w == ["--output-format", "stream-json"]));
    assert!(cmd.args.contains(&"--verbose".to_string()));
    assert!(cmd.args.contains(&"--include-partial-messages".to_string()));
    assert!(cmd.args.windows(2).any(|w| w == ["--model", "haiku"]));

    // Claude at limit (>= 98%) falls back to Codex
    let claude_full = vec![
        Room { tool: Tool::ClaudeCode, installed: true, used_pct: Some(99.0) },
        Room { tool: Tool::Codex, installed: true, used_pct: Some(40.0) },
        Room { tool: Tool::Gemini, installed: true, used_pct: Some(0.0) },
    ];
    assert_eq!(pick(&claude_full, None).unwrap().tool, Tool::Codex);

    // Only Gemini installed -> None (never Gemini)
    let only_gemini = vec![
        Room { tool: Tool::ClaudeCode, installed: false, used_pct: None },
        Room { tool: Tool::Codex, installed: false, used_pct: None },
        Room { tool: Tool::Gemini, installed: true, used_pct: Some(0.0) },
    ];
    assert!(pick(&only_gemini, None).is_none());
    assert!(pick(&only_gemini, Some(Tool::Gemini)).is_none());
}

#[test]
fn claude_stream_reader_emits_from_result_when_no_deltas_or_assistant() {
    // Case 1: Only "result" event arrives (e.g. buffered Windows stream-json)
    let mut r = StreamReader::default();
    assert_eq!(r.line(Tool::ClaudeCode, r#"{"type":"system","subtype":"init"}"#), None);
    let out = r.line(
        Tool::ClaudeCode,
        r#"{"type":"result","subtype":"success","is_error":false,"result":"Use Option::unwrap_or_default here."}"#,
    );
    assert_eq!(out.as_deref(), Some("Use Option::unwrap_or_default here."));
    assert!(r.error.is_none());

    // Case 2: Streaming deltas already emitted -> "result" must NOT duplicate the text
    let mut r2 = StreamReader::default();
    assert_eq!(
        r2.line(Tool::ClaudeCode, r#"{"type":"stream_event","event":{"delta":{"type":"text_delta","text":"Hello"}}}"#).as_deref(),
        Some("Hello")
    );
    assert_eq!(
        r2.line(Tool::ClaudeCode, r#"{"type":"result","subtype":"success","is_error":false,"result":"Hello"}"#),
        None
    );

    // Case 3: Assistant block already emitted -> "result" must NOT duplicate
    let mut r3 = StreamReader::default();
    assert_eq!(
        r3.line(Tool::ClaudeCode, r#"{"type":"assistant","message":{"content":[{"type":"text","text":"Fixed."}]}}"#).as_deref(),
        Some("Fixed.")
    );
    assert_eq!(
        r3.line(Tool::ClaudeCode, r#"{"type":"result","subtype":"success","is_error":false,"result":"Fixed."}"#),
        None
    );
}

#[test]
fn clipboard_only_fix_explain_find_bug_include_copied_text() {
    let err_text = "TypeError: Cannot read properties of undefined (reading 'total')\n    at applyCoupon (checkout.ts:42:18)";
    let ctx = Context {
        selection: None,
        clipboard: Some(err_text.to_string()),
        ..Default::default()
    };
    for action in [Action::Fix, Action::Explain, Action::FindBug] {
        let prompt = build_prompt("", action, &ctx, &[], None);
        assert!(prompt.contains("<copied_text>"), "prompt for {action:?} missing <copied_text>");
        assert!(prompt.contains("applyCoupon (checkout.ts:42:18)"), "prompt for {action:?} missing copied error body");
    }
}

#[test]
fn voice_commands_after_answer_all_match() {
    assert_eq!(voice_intent("paste that", true), Intent::Paste { code_only: false });
    assert_eq!(voice_intent("copy that", true), Intent::Copy);
    assert_eq!(voice_intent("shorter", true), Intent::Redo("shorter".into()));
    assert_eq!(voice_intent("send it to Codex", true), Intent::Send(Tool::Codex));
    assert_eq!(voice_intent("remember that I use Rust", true), Intent::Remember("I use Rust".into()));
    assert_eq!(voice_intent("save this", true), Intent::Save(String::new()));
}
