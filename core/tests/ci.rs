use aiuc_core::ci::{self, CiRun};

const RUNS_JSON: &str = r#"{
  "total_count": 3,
  "workflow_runs": [
    {
      "id": 101,
      "name": "CI",
      "head_branch": "agents/fix-checkout",
      "status": "completed",
      "conclusion": "failure",
      "html_url": "https://github.com/acme/web/actions/runs/101",
      "updated_at": "2026-11-27T12:00:00Z"
    },
    {
      "id": 102,
      "name": "CI",
      "head_branch": "main",
      "status": "completed",
      "conclusion": "failure",
      "html_url": "https://github.com/acme/web/actions/runs/102",
      "updated_at": "2026-11-27T11:55:00Z"
    },
    {
      "id": 103,
      "name": "CI",
      "head_branch": "gemini/autopilot-ci",
      "status": "in_progress",
      "conclusion": null,
      "html_url": "https://github.com/acme/web/actions/runs/103",
      "updated_at": "2026-11-27T12:02:00Z"
    }
  ]
}"#;

#[test]
fn parse_runs_watched_and_newly_failed() {
    let runs = ci::parse_runs(RUNS_JSON, "acme/web");
    assert_eq!(runs.len(), 3);
    assert_eq!(runs[0].id, 101);
    assert_eq!(runs[0].branch, "agents/fix-checkout");
    assert_eq!(runs[0].conclusion.as_deref(), Some("failure"));

    // Watched: agent prefixes + explicit task branches, NOT main
    let task_branches = vec!["feat/custom-task".to_owned()];
    assert!(ci::watched(&runs[0], &task_branches)); // agents/
    assert!(!ci::watched(&runs[1], &task_branches)); // main
    assert!(ci::watched(&runs[2], &task_branches)); // gemini/

    let custom = CiRun {
        id: 104,
        repo: "acme/web".into(),
        branch: "feat/custom-task".into(),
        name: "CI".into(),
        status: "completed".into(),
        conclusion: Some("failure".into()),
        html_url: "https://github.com/acme/web/actions/runs/104".into(),
        updated_at: "2026-11-27T12:05:00Z".into(),
    };
    assert!(ci::watched(&custom, &task_branches));

    // newly_failed ignores already-failed run 101, catches newly failed 104
    let prev = vec![runs[0].clone()];
    let now = vec![runs[0].clone(), custom.clone()];
    let fresh = ci::newly_failed(&prev, &now);
    assert_eq!(fresh, vec![custom]);
}

#[test]
fn log_tail_strips_ansi_and_timestamps_and_takes_last_n_lines() {
    let raw = "\
2026-11-27T12:00:01.1234567Z \x1b[32mRunning tests...\x1b[0m\n\
2026-11-27T12:00:02.0000000Z \x1b[31merror[E0308]: mismatched types\x1b[0m\n\
2026-11-27T12:00:02.1000000Z   --> src/lib.rs:42:9\n\
2026-11-27T12:00:03.0000000Z test result: FAILED. 1 failed\n\n";

    let tail = ci::log_tail(raw, 2);
    assert_eq!(tail, "  --> src/lib.rs:42:9\ntest result: FAILED. 1 failed");

    let all = ci::log_tail(raw, 10);
    assert!(!all.contains("\x1b["));
    assert!(!all.contains("2026-11-27T"));
    assert!(all.contains("error[E0308]: mismatched types"));
}

#[test]
fn owner_repo_extracts_slug() {
    assert_eq!(ci::owner_repo("https://github.com/xyashas/ai-usage-controller.git").as_deref(), Some("xyashas/ai-usage-controller"));
    assert_eq!(ci::owner_repo("xyashas/ai-usage-controller").as_deref(), Some("xyashas/ai-usage-controller"));
    assert_eq!(ci::owner_repo("not-a-repo"), None);
}
