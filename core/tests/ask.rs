use aiuc_core::ask::{self, Action, Context, Mode, Room, StreamReader, Turn};
use aiuc_core::model::Tool;
use std::path::Path;

fn room(tool: Tool, installed: bool, used: Option<f64>) -> Room {
    Room { tool, installed, used_pct: used }
}

#[test]
fn picks_the_installed_tool_with_most_room() {
    let rooms = [room(Tool::ClaudeCode, true, Some(96.0)), room(Tool::Codex, true, Some(30.0)), room(Tool::Gemini, false, Some(5.0))];
    let p = ask::pick(&rooms, None).unwrap();
    assert_eq!(p.tool, Tool::Codex, "Gemini has more room but isn't installed");
    assert_eq!(p.reason, "Claude is at 96%");
    assert_eq!(ask::pick(&rooms, Some(Tool::Codex)).unwrap().reason, "your pick");
    let out = ask::pick(&[room(Tool::ClaudeCode, true, Some(99.0)), room(Tool::Codex, true, Some(40.0))], Some(Tool::ClaudeCode)).unwrap();
    assert_eq!(out.tool, Tool::Codex, "a pinned tool that's out of room is skipped");
    assert_eq!(ask::pick(&[room(Tool::Gemini, true, None)], None).unwrap().reason, "the only one installed");
    assert!(ask::pick(&[room(Tool::Codex, false, Some(1.0))], None).is_none());
}

#[test]
fn prompt_carries_the_context_and_history() {
    let ctx = Context {
        selection: Some("let x = 1".into()),
        window: Some("Code — main.rs".into()),
        screenshot: Some("/tmp/shot.png".into()),
        ..Default::default()
    };
    let history = vec![Turn { question: "what is this".into(), answer: "A variable.".into() }];
    let p = ask::build_prompt("", Action::Explain, &ctx, &history, Some("Limits: Claude 5-hour 40%"));
    for needle in ["<selected_text>\nlet x = 1", "Code — main.rs", "/tmp/shot.png", "User: what is this", "Claude 5-hour 40%", "Explain this clearly"] {
        assert!(p.contains(needle), "missing {needle}");
    }
    let r = ask::build_prompt("", Action::Shorter, &ctx, &[], None);
    assert!(r.contains("ONLY the new text") && !r.contains("<my_ai_work>"));
    assert!(ask::build_prompt("where do I turn off sync?", Action::Point, &Context { screenshot_size: Some((1600, 900)), ..ctx }, &[], None).contains("1600×900"));
    let long = "x".repeat(20000);
    assert!(ask::build_prompt("", Action::Ask, &Context { selection: Some(long), ..Default::default() }, &[], None).contains("more characters]"));
}

#[test]
fn work_questions_are_recognised() {
    assert!(ask::about_work("What are my agents doing?"));
    assert!(ask::about_work("can I start a big refactor before my 3pm"));
    assert!(!ask::about_work("translate this to french"));
    assert!(ask::wants_pointing("Where do I turn off notifications?"));
    assert!(!ask::wants_pointing("Explain closures"));
}

#[test]
fn commands_are_read_only_and_non_interactive() {
    let c = ask::command(Tool::ClaudeCode, Path::new("/bin/claude"), Mode::Quick, "hi", None);
    assert!(c.args.starts_with(&["-p".to_string()]) && c.args.contains(&"haiku".to_string()) && c.args.contains(&"Read".to_string()));
    assert_eq!(c.stdin, "hi");
    let x = ask::command(Tool::Codex, Path::new("codex"), Mode::Deep, "hi", Some(Path::new("/tmp/s.png")));
    assert!(x.args.windows(2).any(|w| w == ["--sandbox", "read-only"]) && x.args.windows(2).any(|w| w == ["-i", "/tmp/s.png"]));
    assert_eq!(x.args.last().unwrap(), "-");
    let g = ask::command(Tool::Gemini, Path::new("gemini"), Mode::Deep, "hi", Some(Path::new("/tmp/s.png")));
    assert!(g.args[1].starts_with("@/tmp/s.png"));
}

#[test]
fn reads_each_clis_stream() {
    let mut r = StreamReader::default();
    let claude = [
        r#"{"type":"system","subtype":"init"}"#,
        r#"{"type":"stream_event","event":{"type":"content_block_delta","delta":{"type":"text_delta","text":"Hel"}}}"#,
        r#"{"type":"stream_event","event":{"type":"content_block_delta","delta":{"type":"text_delta","text":"lo"}}}"#,
        r#"{"type":"assistant","message":{"content":[{"type":"text","text":"Hello"}]}}"#,
        r#"{"type":"result","is_error":false,"result":"Hello"}"#,
    ];
    let out: String = claude.iter().filter_map(|l| r.line(Tool::ClaudeCode, l)).collect();
    assert_eq!(out.trim(), "Hello", "partial deltas aren't repeated by the full message");
    let mut r = StreamReader::default();
    let plain: String = [r#"{"type":"assistant","message":{"content":[{"type":"text","text":"Hi there"}]}}"#].iter().filter_map(|l| r.line(Tool::ClaudeCode, l)).collect();
    assert_eq!(plain, "Hi there");
    let mut r = StreamReader::default();
    assert_eq!(r.line(Tool::ClaudeCode, r#"{"type":"result","is_error":true,"result":"Usage limit reached"}"#), None);
    assert_eq!(r.error.as_deref(), Some("Usage limit reached"));

    let mut r = StreamReader::default();
    let codex_new = [r#"{"type":"thread.started"}"#, r#"{"type":"item.completed","item":{"type":"reasoning","text":"hmm"}}"#, r#"{"type":"item.completed","item":{"type":"agent_message","text":"Use git stash."}}"#];
    assert_eq!(codex_new.iter().filter_map(|l| r.line(Tool::Codex, l)).collect::<String>().trim(), "Use git stash.");
    let mut r = StreamReader::default();
    let codex_old = [r#"{"msg":{"type":"agent_message_delta","delta":"Use "}}"#, r#"{"msg":{"type":"agent_message_delta","delta":"git"}}"#, r#"{"msg":{"type":"agent_message","message":"Use git"}}"#];
    assert_eq!(codex_old.iter().filter_map(|l| r.line(Tool::Codex, l)).collect::<String>(), "Use git");
    assert_eq!(StreamReader::default().line(Tool::Gemini, "plain text"), Some("plain text\n".into()));
}

#[test]
fn points_and_code_are_pulled_out_of_answers() {
    let a = "1. Open Settings\n<point x=\"120\" y=\"40.5\" label=\"Settings\"/>\n\n2. Turn off Sync\n<point x=\"bad\" y=\"1\"/>\nDone.";
    let (pts, text) = ask::parse_points(a);
    assert_eq!(pts, vec![ask::Point { x: 120.0, y: 40.5, label: "Settings".into() }]);
    assert!(!text.contains("<point") && text.contains("2. Turn off Sync") && !text.contains("\n\n\n"));
    assert_eq!(ask::first_code_block("Run:\n```bash\nnpm test -- --watch\n```\nthen"), Some("npm test -- --watch".into()));
    assert_eq!(ask::first_code_block("no code"), None);
    assert!(ask::council_prompt("q", &[(Tool::Codex, "a".into())]).contains("<answer from=\"Codex\">"));
}

#[test]
fn failures_are_classified_so_another_tool_can_answer() {
    use ask::Failure;
    assert_eq!(ask::classify("When using Gemini API, you must specify the GEMINI_API_KEY environment variable."), Failure::SignedOut);
    assert_eq!(ask::classify("Please run /login"), Failure::SignedOut);
    assert_eq!(ask::classify("Claude AI usage limit reached|1700000000"), Failure::OutOfRoom);
    assert_eq!(ask::classify("429 Too Many Requests"), Failure::OutOfRoom);
    assert_eq!(ask::classify("segfault"), Failure::Other);
    assert_eq!(ask::classify("Gemini: An unexpected critical error occurred: This client is no longer supported for Gemini Code Assist for individuals. To continue using Gemini, please migrate to the Antigravity suite of products: https://antigravity.google"), Failure::Unsupported);
    assert!(ask::explain_failure(Tool::Gemini, Failure::Unsupported).contains("GEMINI_API_KEY"));
    assert!(ask::explain_failure(Tool::Gemini, Failure::SignedOut).contains("Login with Google"));
}
