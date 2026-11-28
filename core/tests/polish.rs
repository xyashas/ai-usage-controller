use aiuc_core::polish;
use std::path::Path;
use std::process::Command;

fn git(dir: &Path, args: &[&str]) {
    let status = Command::new("git")
        .current_dir(dir)
        .args(args)
        .env("GIT_AUTHOR_NAME", "test")
        .env("GIT_AUTHOR_EMAIL", "test@example.com")
        .env("GIT_COMMITTER_NAME", "test")
        .env("GIT_COMMITTER_EMAIL", "test@example.com")
        .status()
        .expect("git must run");
    assert!(status.success(), "git {args:?} failed");
}

#[test]
fn gather_reads_branch_changed_files_rules_and_caps_at_15() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();

    git(root, &["init", "-b", "feat/polish-test"]);
    std::fs::write(root.join("README.md"), "# Test repo\n").unwrap();
    let rules_text = (1..=60).map(|i| format!("Rule line {i}")).collect::<Vec<_>>().join("\n");
    std::fs::write(root.join("AGENTS.md"), &rules_text).unwrap();
    git(root, &["add", "."]);
    git(root, &["commit", "-m", "initial"]);

    // Create 18 changed/untracked files to verify the 15-file cap
    for i in 0..18 {
        std::fs::write(root.join(format!("file_{i:02}.txt")), "hello\n").unwrap();
    }

    let ctx = polish::gather(Some(root), Some("  error[E0308]: mismatched types\n"));
    assert!(ctx.repo_name.is_some());
    assert_eq!(ctx.branch.as_deref(), Some("feat/polish-test"));
    assert_eq!(ctx.changed_files.len(), 15);
    assert_eq!(ctx.recent_failure.as_deref(), Some("error[E0308]: mismatched types"));

    let rules = ctx.rules.as_deref().expect("AGENTS.md rules should be loaded");
    assert_eq!(rules.lines().count(), 40);
    assert!(rules.contains("Rule line 1"));
    assert!(!rules.contains("Rule line 41"));

    let req = polish::request("fix the build", &ctx);
    assert!(req.contains("feat/polish-test"));
    assert!(req.contains("error[E0308]: mismatched types"));
    assert!(req.contains("fix the build"));
    assert!(req.contains("ONLY the rewritten prompt"));
}

#[test]
fn gather_handles_none_repo_gracefully() {
    let ctx = polish::gather(None, None);
    assert!(ctx.repo_name.is_none());
    assert!(ctx.branch.is_none());
    assert!(ctx.changed_files.is_empty());
    assert!(ctx.recent_failure.is_none());
    assert!(ctx.rules.is_none());
}

#[test]
fn clean_strips_preambles_code_fences_and_quotes() {
    assert_eq!(
        polish::clean("Here is the rewritten prompt:\n\"Fix the checkout tax calculation and run npm test.\""),
        "Fix the checkout tax calculation and run npm test."
    );
    assert_eq!(
        polish::clean("Here's your polished prompt:\n```markdown\nUpdate core/src/polish.rs and ensure cargo test passes.\n```"),
        "Update core/src/polish.rs and ensure cargo test passes."
    );
    assert_eq!(
        polish::clean("  \u{201c}Add unit tests for watch parsing.\u{201d}  "),
        "Add unit tests for watch parsing."
    );
}
