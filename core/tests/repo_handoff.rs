use aiuc_core::repo_handoff::{self as rh, Latest};
use std::{fs, path::Path, process::Command};

fn git(dir: &Path, args: &[&str]) {
    let ok = Command::new("git").current_dir(dir).args(args).status().unwrap().success();
    assert!(ok, "git {args:?} failed");
}

fn repo() -> tempfile::TempDir {
    let d = tempfile::tempdir().unwrap();
    git(d.path(), &["init", "-q", "-b", "main"]);
    git(d.path(), &["config", "user.email", "t@example.com"]);
    git(d.path(), &["config", "user.name", "T"]);
    git(d.path(), &["config", "commit.gpgsign", "false"]);
    fs::write(d.path().join("README.md"), "hi\n").unwrap();
    git(d.path(), &["add", "."]);
    git(d.path(), &["commit", "-qm", "init"]);
    d
}

#[test]
fn rule_block_is_added_once_and_refreshed_in_place() {
    let once = rh::upsert_block("# Project\n\nNotes.\n");
    assert!(once.starts_with("# Project\n\nNotes.\n\n<!-- aiuc:handoff -->"));
    assert_eq!(rh::upsert_block(&once), once, "idempotent");
    let edited = once.replace("Before you start", "OLD TEXT") + "\nAfter.\n";
    let refreshed = rh::upsert_block(&edited);
    assert!(refreshed.contains("Before you start") && !refreshed.contains("OLD TEXT"));
    assert!(refreshed.ends_with("\nAfter.\n"), "text after the block is kept");
    assert!(rh::upsert_block("").starts_with("<!-- aiuc:handoff -->"));
}

#[test]
fn stop_hook_merges_into_existing_claude_settings() {
    let existing = r#"{"permissions":{"allow":["Bash(ls)"]},"hooks":{"Stop":[{"hooks":[{"type":"command","command":"echo hi"}]}]}}"#;
    let merged = rh::merge_settings(existing).unwrap();
    let v: serde_json::Value = serde_json::from_str(&merged).unwrap();
    assert_eq!(v["permissions"]["allow"][0], "Bash(ls)");
    let stop = v["hooks"]["Stop"].as_array().unwrap();
    assert_eq!(stop.len(), 2, "the user's own hook stays");
    assert!(stop[1]["hooks"][0]["command"].as_str().unwrap().contains(".aiuc/hook.mjs"));
    assert_eq!(rh::merge_settings(&merged).unwrap(), merged, "idempotent");
    assert!(rh::merge_settings("{ not json").is_err(), "broken settings are left alone");
}

#[test]
fn github_urls_in_every_form() {
    for u in ["git@github.com:acme/app.git", "https://github.com/acme/app", "https://github.com/acme/app.git/", "ssh://git@github.com/acme/app.git", "https://x-access-token:abc@github.com/acme/app.git"] {
        assert_eq!(rh::github_url(u).as_deref(), Some("https://github.com/acme/app"), "{u}");
    }
    assert_eq!(rh::github_url("https://gitlab.com/acme/app.git"), None);
}

#[test]
fn setup_commits_only_its_own_files_and_finds_handoffs() {
    let d = repo();
    let root = d.path();
    fs::create_dir(root.join("src")).unwrap();
    fs::write(root.join("src/wip.rs"), "uncommitted work").unwrap();
    assert!(!rh::is_set_up(root));

    let r = rh::setup(&root.join("src"), true).unwrap();
    assert!(r.committed);
    for f in [".aiuc/hook.mjs", "AGENTS.md", "CLAUDE.md", ".claude/settings.json"] {
        assert!(r.changed.contains(&f.to_string()), "{f} in {:?}", r.changed);
    }
    assert!(!r.changed.contains(&"GEMINI.md".to_string()), "no GEMINI.md unless the repo has one");
    assert!(rh::is_set_up(root));
    let status = String::from_utf8(Command::new("git").current_dir(root).args(["status", "--porcelain"]).output().unwrap().stdout).unwrap();
    assert!(status.contains("src/"), "the user's own work isn't committed: {status}");
    assert!(rh::setup(root, true).unwrap().changed.is_empty(), "second run changes nothing");

    // No handoff yet.
    assert_eq!(rh::latest(root).unwrap(), None);

    // A cloud agent commits a handoff on another branch.
    git(root, &["switch", "-qc", "claude/fix-login"]);
    fs::write(root.join(".aiuc/handoff.md"), "# Handoff\n\n## Status\nLogin fixed, tests next.\n").unwrap();
    git(root, &["add", ".aiuc/handoff.md"]);
    git(root, &["commit", "-qm", "handoff"]);
    git(root, &["switch", "-q", "main"]);
    let Latest { branch, origin, content, .. } = rh::latest(root).unwrap().unwrap();
    assert_eq!(branch, "claude/fix-login");
    assert_eq!(origin, "commit");
    assert!(content.contains("tests next"));

    let s = rh::status(root);
    assert!(s.set_up && s.github.is_none() && s.current_branch == "main");

    let p = rh::continue_prompt(&s.name, s.latest.as_ref().unwrap(), true);
    assert!(p.contains("on branch `claude/fix-login`") && p.contains("tests next"));
    assert!(p.contains("Read .aiuc/handoff.md"));
}

#[test]
fn roots_dedupes_folders_of_the_same_repo_and_skips_others() {
    let a = repo();
    fs::create_dir(a.path().join("sub")).unwrap();
    let plain = tempfile::tempdir().unwrap();
    let missing = a.path().join("gone");
    let roots = rh::roots([a.path().join("sub").as_path(), a.path(), plain.path(), missing.as_path()]);
    assert_eq!(roots.len(), 1);
    assert_eq!(fs::canonicalize(&roots[0]).unwrap(), fs::canonicalize(a.path()).unwrap());
}

#[test]
fn hook_script_writes_the_auto_section_and_keeps_notes() {
    if Command::new("node").arg("--version").output().is_err() { return; }
    let d = repo();
    let root = d.path();
    rh::setup(root, false).unwrap();
    let transcript = root.join("t.jsonl");
    let lines = [
        serde_json::json!({"type":"user","message":{"role":"user","content":"Add a login page with token sk-ant-abcdefghijklmnopqrstuv"}}),
        serde_json::json!({"type":"assistant","message":{"role":"assistant","content":[
            {"type":"tool_use","name":"Write","input":{"file_path": root.join("src/login.tsx").to_string_lossy()}},
            {"type":"tool_use","name":"TodoWrite","input":{"todos":[{"content":"Write tests","status":"pending"}]}},
            {"type":"text","text":"Login page added."}]}}),
    ];
    fs::write(&transcript, lines.iter().map(|l| l.to_string()).collect::<Vec<_>>().join("\n")).unwrap();
    fs::create_dir_all(root.join(".aiuc")).unwrap();
    fs::write(root.join(".aiuc/handoff.md"), "# Handoff\n\n## Status\nMy own notes.\n").unwrap();
    let run = || {
        let mut child = Command::new("node").arg(root.join(".aiuc/hook.mjs")).current_dir(root)
            .env("CLAUDE_PROJECT_DIR", root).env_remove("CLAUDE_CODE_REMOTE")
            .stdin(std::process::Stdio::piped()).spawn().unwrap();
        use std::io::Write;
        child.stdin.take().unwrap().write_all(serde_json::json!({"transcript_path": transcript}).to_string().as_bytes()).unwrap();
        assert!(child.wait().unwrap().success());
        fs::read_to_string(root.join(".aiuc/handoff.md")).unwrap()
    };
    let out = run();
    assert!(out.contains("My own notes."), "notes kept: {out}");
    assert!(out.contains("Add a login page") && out.contains("`src/login.tsx`") && out.contains("- [ ] Write tests"));
    assert!(!out.contains("sk-ant-abc"), "secrets redacted");
    assert_eq!(run().matches("<!-- aiuc:auto -->").count(), 1, "rewritten in place");
}
