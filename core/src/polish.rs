//! Prompt polish: gather local repo context and build/clean an AI rewrite
//! request that turns a rough prompt into a specific, actionable one.

use serde::Serialize;
use std::collections::BTreeSet;
use std::path::Path;
use std::process::{Command, Stdio};

const MAX_CHANGED_FILES: usize = 15;
const MAX_RULE_LINES: usize = 40;

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct PolishContext {
    pub repo_name: Option<String>,
    pub branch: Option<String>,
    pub changed_files: Vec<String>,
    pub recent_failure: Option<String>,
    pub rules: Option<String>,
}

fn git(root: &Path, args: &[&str]) -> Option<String> {
    let mut cmd = Command::new("git");
    cmd.current_dir(root)
        .args(args)
        .stdin(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x08000000);
    }
    let out = cmd.output().ok()?;
    if !out.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&out.stdout).into_owned())
}

fn read_rules(root: &Path) -> Option<String> {
    for name in ["AGENTS.md", "CLAUDE.md"] {
        let p = root.join(name);
        if let Ok(content) = std::fs::read_to_string(&p) {
            let lines: Vec<&str> = content.lines().take(MAX_RULE_LINES).collect();
            let joined = lines.join("\n").trim().to_owned();
            if !joined.is_empty() {
                return Some(joined);
            }
        }
    }
    None
}

/// Gather lightweight local context from `repo_root` and an optional failing
/// test output tail. Everything stays local and is never persisted.
pub fn gather(repo_root: Option<&Path>, failure_tail: Option<&str>) -> PolishContext {
    let recent_failure = failure_tail
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned);

    let Some(root) = repo_root else {
        return PolishContext {
            recent_failure,
            ..Default::default()
        };
    };

    let repo_name = root
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .filter(|s| !s.is_empty());

    let branch = git(root, &["rev-parse", "--abbrev-ref", "HEAD"])
        .map(|s| s.trim().to_owned())
        .filter(|s| !s.is_empty() && s != "HEAD");

    let mut files = BTreeSet::new();
    for args in [&["diff", "--name-only"][..], &["diff", "--cached", "--name-only"][..]] {
        if let Some(out) = git(root, args) {
            for line in out.lines().map(str::trim).filter(|l| !l.is_empty()) {
                files.insert(line.to_owned());
            }
        }
    }
    if let Some(status) = git(root, &["status", "--porcelain"]) {
        for line in status.lines() {
            if line.len() > 3 {
                let path = line[3..].trim();
                let path = path.rsplit_once(" -> ").map(|(_, dst)| dst).unwrap_or(path);
                let path = path.trim_matches('"');
                if !path.is_empty() {
                    files.insert(path.to_owned());
                }
            }
        }
    }
    let changed_files: Vec<String> = files.into_iter().take(MAX_CHANGED_FILES).collect();
    let rules = read_rules(root);

    PolishContext {
        repo_name,
        branch,
        changed_files,
        recent_failure,
        rules,
    }
}

/// Build the instruction for an AI to rewrite `raw` into a specific,
/// actionable prompt grounded in `ctx`.
pub fn request(raw: &str, ctx: &PolishContext) -> String {
    let mut out = String::from(
        "Rewrite the user's coding prompt so it is clear, specific, and ready for a coding agent.\n\
         Rules:\n\
         - Keep the user's exact intent and scope; do not invent extra features.\n\
         - Weave in only the relevant project context below (repo, branch, changed files, recent test failure, project rules).\n\
         - State clearly what \"done\" means (for example, relevant tests and build checks pass).\n\
         - Keep it concise and plain.\n\
         - Reply with ONLY the rewritten prompt — no preamble, no quotes, no markdown code fences.\n\n",
    );

    out.push_str("Context:\n");
    if let Some(repo) = &ctx.repo_name {
        out.push_str(&format!("- Repo: {repo}\n"));
    }
    if let Some(branch) = &ctx.branch {
        out.push_str(&format!("- Branch: {branch}\n"));
    }
    if !ctx.changed_files.is_empty() {
        out.push_str(&format!("- Changed files: {}\n", ctx.changed_files.join(", ")));
    }
    if let Some(fail) = &ctx.recent_failure {
        out.push_str(&format!("- Recent test failure:\n{fail}\n"));
    }
    if let Some(rules) = &ctx.rules {
        out.push_str(&format!("- Project rules (excerpt):\n{rules}\n"));
    }
    if ctx.repo_name.is_none()
        && ctx.branch.is_none()
        && ctx.changed_files.is_empty()
        && ctx.recent_failure.is_none()
        && ctx.rules.is_none()
    {
        out.push_str("- (none)\n");
    }

    out.push_str("\nUser prompt:\n");
    out.push_str(raw.trim());
    out
}

/// Strip common AI wrappers from a rewritten prompt: preambles ("Here is..."),
/// markdown code fences, and outer quotes.
pub fn clean(output: &str) -> String {
    let mut s = output.trim().to_owned();

    // Strip a leading preamble line such as "Here is the rewritten prompt:"
    for _ in 0..2 {
        if let Some((first, rest)) = s.split_once('\n') {
            let f = first.trim().to_ascii_lowercase();
            let is_preamble = f.ends_with(':')
                && (f.starts_with("here is")
                    || f.starts_with("here's")
                    || f.starts_with("here’s")
                    || f.starts_with("sure")
                    || f.starts_with("rewritten")
                    || f.starts_with("polished")
                    || f.starts_with("updated prompt")
                    || f.starts_with("prompt"));
            if is_preamble {
                s = rest.trim().to_owned();
                continue;
            }
        }
        break;
    }

    // Strip outer markdown code fence (``` or ```text / ```markdown)
    if let Some(after_open) = s.strip_prefix("```") {
        if let Some((_, body)) = after_open.split_once('\n') {
            if let Some(inner) = body.trim_end().strip_suffix("```") {
                s = inner.trim().to_owned();
            }
        }
    }

    // Strip matching outer quotes
    let pairs = [('"', '"'), ('\'', '\''), ('\u{201c}', '\u{201d}')];
    for (open, close) in pairs {
        if s.starts_with(open) && s.ends_with(close) && s.len() >= open.len_utf8() + close.len_utf8() {
            let inner = &s[open.len_utf8()..s.len() - close.len_utf8()];
            if !inner.is_empty() {
                s = inner.trim().to_owned();
                break;
            }
        }
    }

    s.trim().to_owned()
}
