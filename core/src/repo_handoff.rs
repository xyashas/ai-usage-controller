//! Repo-based handoff: works for cloud and local sessions alike.
//!
//! Every agent, cloud or local, works on a git repository, so the repository is
//! where a handoff lives. Setting a repo up adds:
//! - `.aiuc/hook.mjs`, which Claude Code runs after every turn (a Stop hook in
//!   `.claude/settings.json`) to keep the auto section of `.aiuc/handoff.md` current;
//! - a short rule in `AGENTS.md` and `CLAUDE.md` (and `GEMINI.md` if present):
//!   read `.aiuc/handoff.md` first, keep its Status current, commit it with the work.
//!
//! The file travels with the branch. The widget finds the newest handoff on any
//! branch (after a `git fetch`), and opens the next tool on it.

use anyhow::{anyhow, bail, ensure, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{Duration, Instant},
};

pub const DIR: &str = ".aiuc";
pub const FILE: &str = ".aiuc/handoff.md";
pub const HOOK: &str = ".aiuc/hook.mjs";
pub const HOOK_SOURCE: &str = include_str!("../data/aiuc-hook.mjs");
const HOOK_COMMAND: &str = "node \"$CLAUDE_PROJECT_DIR/.aiuc/hook.mjs\" || true";
const BEGIN: &str = "<!-- aiuc:handoff -->";
const END: &str = "<!-- /aiuc:handoff -->";

pub const RULE: &str = "## Handoff between AI agents

This repo uses AI Usage Controller handoffs, so any agent (Claude Code, Codex, Gemini; cloud or local) can continue another's work.

- Before you start, read `.aiuc/handoff.md` if it exists. It says what the previous session was doing and what is left.
- Before you finish a task, update the **Status** section of `.aiuc/handoff.md`: what is done, what is next, open problems. Keep it short. Don't touch the auto section at the bottom.
- Commit `.aiuc/handoff.md` together with your work. Never put secrets in it.";

fn block() -> String { format!("{BEGIN}\n{RULE}\n{END}") }

/// Add or refresh the marked rule block in an instructions file.
pub fn upsert_block(existing: &str) -> String {
    let b = block();
    if let (Some(a), Some(z)) = (existing.find(BEGIN), existing.find(END)) {
        if z > a {
            return format!("{}{}{}", &existing[..a], b, &existing[z + END.len()..]);
        }
    }
    if existing.trim().is_empty() { return format!("{b}\n"); }
    format!("{}\n\n{b}\n", existing.trim_end())
}

/// Add the Stop hook to Claude Code's project settings, keeping everything else.
pub fn merge_settings(existing: &str) -> Result<String> {
    let mut v: Value = if existing.trim().is_empty() { json!({}) } else {
        serde_json::from_str(existing).context(".claude/settings.json isn't valid JSON, so it was left alone")?
    };
    ensure!(v.is_object(), ".claude/settings.json isn't a JSON object");
    let present = v["hooks"]["Stop"].as_array().is_some_and(|groups| groups.iter().any(|g| {
        g["hooks"].as_array().is_some_and(|hs| hs.iter().any(|h| h["command"].as_str().is_some_and(|c| c.contains(".aiuc/hook.mjs"))))
    }));
    if !present {
        if !v["hooks"].is_object() { v["hooks"] = json!({}); }
        if !v["hooks"]["Stop"].is_array() { v["hooks"]["Stop"] = json!([]); }
        v["hooks"]["Stop"].as_array_mut().unwrap().push(json!({ "hooks": [{ "type": "command", "command": HOOK_COMMAND, "timeout": 20 }] }));
    }
    Ok(serde_json::to_string_pretty(&v)? + "\n")
}

#[derive(Debug, Clone, Serialize)]
pub struct SetupResult {
    pub root: PathBuf,
    pub changed: Vec<String>,
    pub committed: bool,
}

fn write_if_changed(root: &Path, rel: &str, contents: &str, changed: &mut Vec<String>) -> Result<()> {
    let path = root.join(rel);
    if fs::read_to_string(&path).ok().as_deref() == Some(contents) { return Ok(()); }
    if let Some(parent) = path.parent() { fs::create_dir_all(parent)?; }
    fs::write(&path, contents).with_context(|| format!("Could not write {rel}"))?;
    changed.push(rel.to_owned());
    Ok(())
}

/// The top folder of the git repository that holds `folder`.
pub fn repo_root(folder: &Path) -> Result<PathBuf> {
    let out = git(folder, &["rev-parse", "--show-toplevel"]).map_err(|_| anyhow!("That folder isn't in a git repository. Cloud agents work on GitHub repos, so pick a folder that is one."))?;
    // Git prints forward slashes on Windows too.
    #[cfg(windows)]
    let out = out.replace('/', "\\");
    Ok(PathBuf::from(out.trim()))
}

/// Set a repository up for handoffs. With `commit`, the changed files (and only
/// those) are committed on the current branch. Nothing is ever pushed.
pub fn setup(folder: &Path, commit: bool) -> Result<SetupResult> {
    let root = repo_root(folder)?;
    let mut changed = Vec::new();
    write_if_changed(&root, HOOK, HOOK_SOURCE, &mut changed)?;
    for name in ["AGENTS.md", "CLAUDE.md", "GEMINI.md"] {
        let path = root.join(name);
        let existing = fs::read_to_string(&path).unwrap_or_default();
        if name == "GEMINI.md" && existing.is_empty() { continue; }
        write_if_changed(&root, name, &upsert_block(&existing), &mut changed)?;
    }
    let settings = fs::read_to_string(root.join(".claude/settings.json")).unwrap_or_default();
    write_if_changed(&root, ".claude/settings.json", &merge_settings(&settings)?, &mut changed)?;
    let mut committed = false;
    if commit && !changed.is_empty() {
        let mut add = vec!["add", "--"];
        add.extend(changed.iter().map(String::as_str));
        git(&root, &add)?;
        let mut args = vec!["commit", "--quiet", "-m", "Set up AI handoffs (.aiuc)", "--"];
        args.extend(changed.iter().map(String::as_str));
        committed = git(&root, &args).is_ok();
    }
    Ok(SetupResult { root, changed, committed })
}

pub fn is_set_up(root: &Path) -> bool {
    root.join(HOOK).is_file()
        && fs::read_to_string(root.join("AGENTS.md")).is_ok_and(|s| s.contains(BEGIN))
}

/// The newest handoff in a repository, on any local or fetched branch, or in
/// the working tree if that's newer than anything committed.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Latest {
    pub branch: String,
    /// Seconds since the epoch.
    pub updated_at: i64,
    /// "commit" (on a branch) or "working tree" (not committed yet).
    pub origin: String,
    pub content: String,
}

pub fn latest(root: &Path) -> Result<Option<Latest>> {
    let mut best: Option<Latest> = None;
    let log = git(root, &["log", "--all", "-n", "1", "--format=%ct %H", "--", FILE]).unwrap_or_default();
    if let Some((ts, hash)) = log.trim().split_once(' ') {
        let ts: i64 = ts.parse().unwrap_or(0);
        let branches = git(root, &["branch", "-a", "--contains", hash, "--format=%(refname:short)"]).unwrap_or_default();
        let names: Vec<&str> = branches.lines().map(str::trim).filter(|b| !b.is_empty() && !b.ends_with("/HEAD")).collect();
        // Prefer the remote branch name: that's what a cloud agent pushed.
        let pick = names.iter().find(|b| b.starts_with("origin/")).or(names.first()).copied().unwrap_or("");
        let branch = pick.strip_prefix("origin/").unwrap_or(pick).to_owned();
        let content = git(root, &["show", &format!("{hash}:{FILE}")])?;
        best = Some(Latest { branch, updated_at: ts, origin: "commit".into(), content });
    }
    let local = root.join(FILE);
    if let Ok(meta) = fs::metadata(&local) {
        let mtime = meta.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_secs() as i64).unwrap_or(0);
        if best.as_ref().is_none_or(|b| mtime > b.updated_at + 5) {
            let branch = git(root, &["rev-parse", "--abbrev-ref", "HEAD"]).map(|s| s.trim().to_owned()).unwrap_or_default();
            best = Some(Latest { branch, updated_at: mtime, origin: "working tree".into(), content: fs::read_to_string(&local)? });
        }
    }
    Ok(best)
}

/// Fetch every branch from origin, so handoffs pushed by cloud agents are
/// visible. Never prompts for credentials; gives up after 25 seconds.
pub fn fetch(root: &Path) -> Result<()> {
    git_with_timeout(root, &["fetch", "--quiet", "--prune", "origin"], Duration::from_secs(25)).map(|_| ())
}

/// What the widget shows for one repository.
#[derive(Debug, Clone, Serialize)]
pub struct RepoStatus {
    /// What the UI passes back: the folder path, or `github:owner/name` for a
    /// repository that's only on GitHub.
    pub id: String,
    /// Cloned on this computer.
    pub local: bool,
    pub root: PathBuf,
    /// Folder name, or `owner/name` when origin is on GitHub.
    pub name: String,
    pub set_up: bool,
    /// The branch checked out locally.
    pub current_branch: String,
    /// The GitHub page of `origin`, if it is on GitHub. Cloud agents need it.
    pub github: Option<String>,
    pub latest: Option<Latest>,
}

/// `https://github.com/owner/repo` from an origin URL in any of git's forms.
pub fn github_url(remote: &str) -> Option<String> {
    let r = remote.trim().trim_end_matches('/');
    let rest = r.strip_prefix("git@github.com:")
        .or_else(|| r.strip_prefix("ssh://git@github.com/"))
        .or_else(|| r.strip_prefix("https://github.com/"))
        .or_else(|| r.strip_prefix("http://github.com/"))
        .or_else(|| r.split_once("@github.com/").map(|(_, rest)| rest))?;
    let rest = rest.strip_suffix(".git").unwrap_or(rest);
    let mut parts = rest.split('/');
    let (owner, name) = (parts.next()?, parts.next()?);
    (!owner.is_empty() && !name.is_empty() && parts.next().is_none()).then(|| format!("https://github.com/{owner}/{name}"))
}

pub fn status(root: &Path) -> RepoStatus {
    let github = git(root, &["remote", "get-url", "origin"]).ok().and_then(|u| github_url(&u));
    let folder = root.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let name = github.as_deref().and_then(|g| g.strip_prefix("https://github.com/")).map(str::to_owned).unwrap_or(folder);
    let current_branch = git(root, &["rev-parse", "--abbrev-ref", "HEAD"]).map(|s| s.trim().to_owned()).unwrap_or_default();
    RepoStatus { id: root.to_string_lossy().into_owned(), local: true, root: root.to_owned(), name, set_up: is_set_up(root), current_branch, github, latest: latest(root).ok().flatten() }
}

/// A repository that's on GitHub but not cloned here.
pub fn remote_status(repo: &crate::github::GhRepo) -> RepoStatus {
    RepoStatus {
        id: format!("github:{}", repo.full_name), local: false, root: PathBuf::new(), name: repo.full_name.clone(),
        set_up: false, current_branch: repo.default_branch.clone(), github: Some(repo.html_url.clone()), latest: None,
    }
}

/// Distinct repository roots for these folders, in order. Folders that aren't
/// in a git repository (or no longer exist) are skipped.
pub fn roots<'a>(folders: impl IntoIterator<Item = &'a Path>) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = Vec::new();
    for f in folders {
        if !f.is_dir() { continue; }
        // A folder inside a repo we already have needs no git call.
        if out.iter().any(|r| f.starts_with(r)) { continue; }
        if let Ok(root) = repo_root(f) {
            if !out.contains(&root) { out.push(root); }
        }
    }
    out
}

/// The prompt the next agent starts with.
pub fn continue_prompt(repo_name: &str, latest: &Latest, cloud: bool) -> String {
    let branch = if latest.branch.is_empty() || latest.branch == "HEAD" { String::new() } else { format!(" on branch `{}`", latest.branch) };
    let mut p = format!("Continue the work in {repo_name}{branch}. ");
    if cloud {
        p.push_str("Work on that branch. ");
    } else if !branch.is_empty() {
        p.push_str("Fetch and switch to that branch first if you're not on it. ");
    }
    p.push_str("Read .aiuc/handoff.md, then pick up where the previous session stopped and keep the handoff's Status current.\n\nHere is the handoff, in case the file isn't there yet:\n\n");
    let content = latest.content.trim();
    let clipped: String = content.chars().take(12_000).collect();
    p.push_str(&clipped);
    p
}

/// Run git without a console window or credential prompts.
pub fn git(dir: &Path, args: &[&str]) -> Result<String> {
    git_with_timeout(dir, args, Duration::from_secs(15))
}

/// Clone `owner/name` into `parent/name`. The token is used for the download
/// only; the saved remote is the plain GitHub URL.
pub fn clone(parent: &Path, full_name: &str, token: Option<&str>) -> Result<PathBuf> {
    let name = full_name.rsplit('/').next().filter(|n| !n.is_empty() && !n.starts_with('.')).ok_or_else(|| anyhow!("Not a repository name"))?;
    let dest = parent.join(name);
    ensure!(!dest.exists(), "{} already exists. Add it with + instead.", dest.display());
    let plain = format!("https://github.com/{full_name}.git");
    let url = token.map(|t| format!("https://x-access-token:{t}@github.com/{full_name}.git")).unwrap_or_else(|| plain.clone());
    let dest_s = dest.to_string_lossy().into_owned();
    git_with_timeout(parent, &["clone", "--quiet", &url, &dest_s], Duration::from_secs(600))
        .map_err(|e| anyhow!("{}", e.to_string().replace(token.unwrap_or("\u{0}"), "***")))?;
    git(&dest, &["remote", "set-url", "origin", &plain])?;
    Ok(dest)
}

fn git_with_timeout(dir: &Path, args: &[&str], timeout: Duration) -> Result<String> {
    let exe = crate::handoff::executable("git").unwrap_or_else(|| PathBuf::from("git"));
    let mut cmd = Command::new(exe);
    cmd.current_dir(dir).args(args)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GCM_INTERACTIVE", "never")
        .stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x08000000);
    }
    let mut child = cmd.spawn().map_err(|_| anyhow!("Git isn't installed or couldn't start."))?;
    // Read output on threads so a chatty command can't fill the pipe and stall.
    let mut out = child.stdout.take().unwrap();
    let mut err = child.stderr.take().unwrap();
    let out_t = std::thread::spawn(move || { let mut s = Vec::new(); let _ = std::io::Read::read_to_end(&mut out, &mut s); s });
    let err_t = std::thread::spawn(move || { let mut s = Vec::new(); let _ = std::io::Read::read_to_end(&mut err, &mut s); s });
    let start = Instant::now();
    let status = loop {
        if let Some(s) = child.try_wait()? { break s; }
        if start.elapsed() > timeout {
            let _ = child.kill();
            let _ = child.wait();
            bail!("git {} took too long", args.first().unwrap_or(&""));
        }
        std::thread::sleep(Duration::from_millis(25));
    };
    let stdout = out_t.join().unwrap_or_default();
    let stderr = err_t.join().unwrap_or_default();
    if !status.success() {
        let msg = String::from_utf8_lossy(&stderr);
        bail!("git {}: {}", args.first().unwrap_or(&""), msg.lines().next().unwrap_or("failed"));
    }
    Ok(String::from_utf8_lossy(&stdout).into_owned())
}
