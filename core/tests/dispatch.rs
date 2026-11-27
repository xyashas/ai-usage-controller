use aiuc_core::dispatch::{self, route, split, TaskPlan, ToolRoom};
use aiuc_core::model::{EventKind, Tool, UsageEvent};
use aiuc_core::store::TaskRow;
use aiuc_core::Engine;
use chrono::{Duration, Utc};
use std::collections::BTreeMap;
use std::{fs, path::Path, process::Command};

fn t(text: &str, tool: Option<Tool>) -> TaskPlan {
    TaskPlan { text: text.into(), tool }
}

#[test]
fn splits_a_spoken_request_into_routed_tasks() {
    assert_eq!(
        split("Have Codex fix the failing checkout test, and Claude update the docs."),
        vec![t("Fix the failing checkout test", Some(Tool::Codex)), t("Update the docs.", Some(Tool::ClaudeCode))]
    );
    assert_eq!(
        split("ask claude code to refactor the auth module then gemini: write a changelog"),
        vec![t("Refactor the auth module", Some(Tool::ClaudeCode)), t("Write a changelog", Some(Tool::Gemini))]
    );
    // "and" without a tool named after it is part of one task.
    assert_eq!(split("Fix the login bug and add tests"), vec![t("Fix the login bug and add tests", None)]);
    // A tool named at the end.
    assert_eq!(split("Write the migration script in Codex"), vec![t("Write the migration script", Some(Tool::Codex))]);
    // Lines and bullets are separate tasks.
    assert_eq!(
        split("- bump the version\n2. use Gemini to summarise open issues\n\n"),
        vec![t("Bump the version", None), t("Summarise open issues", Some(Tool::Gemini))]
    );
    // Talking about tools isn't a directive.
    assert_eq!(split("Compare Claude and Codex output formats"), vec![t("Compare Claude and Codex output formats", None)]);
    assert!(split("  \n ; ").is_empty());
    assert_eq!(split("Claude"), vec![t("Claude", None)]);
}

fn room(tool: Tool, used: Option<f64>, ready: bool) -> ToolRoom {
    ToolRoom { tool, ready, used_pct: used, active: false }
}

#[test]
fn routes_to_the_most_room_and_spreads_parallel_work() {
    let rooms = [room(Tool::ClaudeCode, Some(40.0), true), room(Tool::Codex, Some(35.0), true), room(Tool::Gemini, None, false)];
    let r = route(&[t("a", None), t("b", None), t("c", Some(Tool::Gemini))], &rooms);
    assert_eq!(r[0].tool, Some(Tool::Codex));
    assert_eq!(r[0].reason, "Most room: 35% used");
    assert_eq!(r[1].tool, Some(Tool::ClaudeCode), "the second task goes to the other tool");
    assert!(r[2].explicit && r[2].reason.contains("isn't installed"));

    // A nearly used-up tool is only picked when nothing else is ready.
    let low = [room(Tool::ClaudeCode, Some(97.0), true), room(Tool::Codex, Some(70.0), true)];
    assert_eq!(route(&[t("x", None)], &low)[0].tool, Some(Tool::Codex));
    let none = [room(Tool::ClaudeCode, None, false)];
    assert_eq!(route(&[t("x", None)], &none)[0].tool, None);
}

#[test]
fn slugs_are_short_and_branch_safe() {
    assert_eq!(dispatch::slug("Fix the failing checkout test, please!"), "fix-failing-checkout-test-please");
    assert_eq!(dispatch::slug("Ünïcode ✨"), "ncode");
    assert_eq!(dispatch::slug("!!!"), "task");
}

fn git(dir: &Path, args: &[&str]) {
    assert!(Command::new("git").current_dir(dir).args(args).status().unwrap().success(), "git {args:?}");
}

#[test]
fn each_task_gets_its_own_worktree_and_branch() {
    let parent = tempfile::tempdir().unwrap();
    let root = parent.path().join("shop");
    fs::create_dir(&root).unwrap();
    git(&root, &["init", "-q", "-b", "main"]);
    git(&root, &["-c", "user.email=t@e.com", "-c", "user.name=T", "commit", "-q", "--allow-empty", "-m", "init"]);
    let (a, ba) = dispatch::create_worktree(&root, "Fix checkout").unwrap();
    let (b, bb) = dispatch::create_worktree(&root, "Fix checkout").unwrap();
    assert_eq!(ba, "agents/fix-checkout");
    assert_eq!(bb, "agents/fix-checkout-2");
    assert!(a.ends_with("shop.agents/fix-checkout") && a.is_dir());
    assert!(b.is_dir() && a != b);
}

#[test]
fn agent_runs_follow_the_session_in_the_task_folder() {
    let eng = Engine::in_memory().unwrap();
    let now = Utc::now();
    let row = |cwd: &str, tool, mins| TaskRow { id: 0, created_at: now - Duration::minutes(mins), repo: "/dev/shop".into(), cwd: cwd.into(), branch: Some("agents/x".into()), tool, text: "Fix it".into() };
    let working = eng.record_task(&row("/dev/shop.agents/a", Tool::Codex, 10)).unwrap();
    let idle = eng.record_task(&row("/dev/shop.agents/b", Tool::ClaudeCode, 30)).unwrap();
    let fresh = eng.record_task(&row("/dev/shop.agents/c", Tool::Codex, 1)).unwrap();
    let stale = eng.record_task(&row("/dev/shop.agents/d", Tool::Codex, 20)).unwrap();
    let gone = eng.record_task(&row("/dev/shop.agents/e", Tool::Codex, 20)).unwrap();
    eng.dismiss_task(gone).unwrap();

    let ev = |tool, cwd: &str, ago_s: i64, cost: f64| {
        let mut e = UsageEvent::new(tool, EventKind::Assistant, format!("{cwd}{ago_s}"), now - Duration::seconds(ago_s), "s1".into());
        e.project = Some(cwd.into());
        e.cost_usd = cost;
        e
    };
    let mut recent = BTreeMap::new();
    recent.insert(Tool::Codex, vec![ev(Tool::Codex, "/dev/shop.agents/a", 300, 0.5), ev(Tool::Codex, "/dev/shop.agents/a/", 20, 0.25), ev(Tool::Codex, "/elsewhere", 5, 9.0)]);
    recent.insert(Tool::ClaudeCode, vec![ev(Tool::ClaudeCode, "/dev/shop.agents/b", 600, 1.0)]);
    let runs = eng.agent_runs(now, &recent).unwrap();
    let state = |id| runs.iter().find(|r| r.id == id).map(|r| r.state.as_str());
    assert_eq!(state(working), Some("working"));
    assert_eq!(state(idle), Some("idle"));
    assert_eq!(state(fresh), Some("starting"));
    assert_eq!(state(stale), Some("no_activity"));
    assert_eq!(state(gone), None, "dismissed");
    let w = runs.iter().find(|r| r.id == working).unwrap();
    assert_eq!((w.requests, w.cost_usd, w.repo_name.as_str()), (2, 0.75, "shop"));
    assert_eq!(runs[0].id, fresh, "newest first");
}
