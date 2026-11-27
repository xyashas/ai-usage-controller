mod agents;
mod ask;
mod autosend;
mod buddy;
mod capture;
mod clipboard_watch;
mod watch;
mod claude_web;
mod dispatch;
mod hover;
mod handoff;
mod voice;
mod window;

use aiuc_core::model::Tool;
use aiuc_core::plans::Plan;
use aiuc_core::settings::Settings;
use aiuc_core::{Engine, Snapshot};
use chrono::{Duration as CDuration, Utc};
use serde::Serialize;
use std::collections::{BTreeMap, HashSet};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Emitter, State};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};
use tauri_plugin_autostart::ManagerExt as AutostartExt;
use tauri_plugin_notification::NotificationExt;
use tauri_plugin_opener::OpenerExt;

/// Opens the panel on the Agents tab with the task box focused.
const DISPATCH_HOTKEY: &str = "Alt+Shift+A";
const TICK: Duration = Duration::from_secs(3);
const LIVE_EVERY: Duration = Duration::from_secs(60);
const SYNC_EVERY: Duration = Duration::from_secs(60);
const TABLES_EVERY: Duration = Duration::from_secs(6 * 3600);
static CLAUDE_CODE_LOGIN: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

struct PendingHandoff {
    id: u64,
    /// The project folder on this computer, if there is one.
    cwd: Option<PathBuf>,
    target: Tool,
}

pub fn tool_name(t: Tool) -> &'static str {
    match t { Tool::ClaudeCode => "Claude Code", Tool::Codex => "Codex", Tool::Gemini => "Gemini" }
}

struct AppState {
    handoff: Arc<Mutex<Option<PendingHandoff>>>,
    engine: Arc<Mutex<Engine>>,
    last: Arc<Mutex<Option<Snapshot>>>,
    hit: Arc<Mutex<window::Rect>>,
    web: Arc<claude_web::ClaudeWeb>,
}

type CmdResult<T> = Result<T, String>;
fn err<E: std::fmt::Display>(e: E) -> String {
    e.to_string()
}

fn snapshot_now(state: &AppState) -> CmdResult<Snapshot> {
    let mut eng = state.engine.lock().map_err(err)?;
    let s = eng.snapshot(Utc::now()).map_err(err)?;
    *state.last.lock().map_err(err)? = Some(s.clone());
    Ok(s)
}

// ---------------- commands ----------------

#[tauri::command]
fn get_snapshot(state: State<AppState>) -> CmdResult<Snapshot> {
    snapshot_now(&state)
}

#[tauri::command]
fn dismiss_daily_recap(state: State<AppState>) -> CmdResult<()> {
    state.engine.lock().map_err(err)?.dismiss_daily_recap(Utc::now()).map_err(err)
}

#[derive(Serialize)]
struct Preview {
    id: u64,
    prompt: String,
    /// Where the handoff can open, best first.
    destinations: Vec<handoff::DestOption>,
    /// The folder or repository it continues, for the preview's subtitle.
    place: String,
}

fn new_preview(pending: &Mutex<Option<PendingHandoff>>, cwd: Option<PathBuf>, target: Tool, prompt: String, place: String) -> CmdResult<Preview> {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    let id = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let destinations = handoff::destinations(target, cwd.is_some());
    *pending.lock().map_err(err)? = Some(PendingHandoff { id, cwd, target });
    Ok(Preview { id, prompt, destinations, place })
}

#[tauri::command]
async fn preview_handoff(window: tauri::WebviewWindow, state: State<'_, AppState>, source: Tool, session: String, target: Tool, test: Option<bool>) -> CmdResult<Preview> {
    if window.label() != "widget" { return Err("Open the widget to preview a handoff.".into()); }
    let engine = state.engine.clone();
    let pending = state.handoff.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let roots = {
            let mut eng = engine.lock().map_err(err)?;
            // Test mode (Check setup) offers the latest session whatever the usage;
            // otherwise the offer must come from the live snapshot.
            let offers = if test.unwrap_or(false) {
                eng.test_handoff_offers(Utc::now()).map_err(err)?
            } else {
                eng.snapshot(Utc::now()).map_err(err)?.handoffs
            };
            if !offers.iter().any(|o| o.source == source && o.session == session && o.targets.contains(&target)) {
                return Err("The session or available destination changed.".into());
            }
            eng.roots(source)
        };
        let (cwd, prompt) = aiuc_core::handoff::prepare(&roots, source, &session).map_err(err)?;
        let place = cwd.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        new_preview(&pending, Some(cwd), target, prompt, place)
    }).await.map_err(err)?
}

#[tauri::command]
fn test_handoff_offers(state: State<AppState>) -> CmdResult<Vec<aiuc_core::snapshot::HandoffOffer>> {
    state.engine.lock().map_err(err)?.test_handoff_offers(Utc::now()).map_err(err)
}

#[tauri::command]
async fn launch_handoff(window: tauri::WebviewWindow, state: State<'_, AppState>, id: u64, prompt: String, dest: handoff::Dest) -> CmdResult<String> {
    if window.label() != "widget" { return Err("Open the widget to launch a handoff.".into()); }
    let pending = state.handoff.clone();
    let engine = state.engine.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let mut guard = pending.lock().map_err(err)?;
        let draft = guard.as_ref().filter(|d| d.id == id).ok_or("Preview this session again.")?;
        let auto = engine.lock().map_err(err)?.settings.auto_send;
        let msg = handoff::launch(draft.cwd.as_deref(), draft.target, dest, &prompt, auto)?;
        if let Ok(eng) = engine.lock() { let _ = eng.record_handoff(Utc::now()); }
        *guard = None;
        Ok(msg)
    }).await.map_err(err)?
}

// ---------------- repo handoffs (cloud and local) ----------------

use aiuc_core::repo_handoff as rh;

/// Repositories to show: the ones the user added, then those of recent sessions.
pub(crate) fn repo_candidates(engine: &Mutex<Engine>) -> CmdResult<Vec<PathBuf>> {
    let eng = engine.lock().map_err(err)?;
    let mut folders = eng.settings.repos.clone();
    folders.extend(eng.recent_projects(Utc::now()).map_err(err)?);
    drop(eng);
    let mut roots = rh::roots(folders.iter().map(PathBuf::as_path));
    roots.truncate(8);
    Ok(roots)
}

async fn blocking<T: Send + 'static>(f: impl FnOnce() -> CmdResult<T> + Send + 'static) -> CmdResult<T> {
    tauri::async_runtime::spawn_blocking(f).await.map_err(err)?
}

#[tauri::command]
async fn list_repos(state: State<'_, AppState>, fetch: Option<bool>) -> CmdResult<Vec<rh::RepoStatus>> {
    let engine = state.engine.clone();
    blocking(move || {
        let roots = repo_candidates(&engine)?;
        if fetch.unwrap_or(false) {
            // Handoffs pushed by cloud agents arrive with a fetch. In parallel,
            // and failures (offline, no access) just leave the local view.
            let handles: Vec<_> = roots.iter().filter(|r| rh::is_set_up(r)).cloned()
                .map(|r| std::thread::spawn(move || { let _ = rh::fetch(&r); })).collect();
            for h in handles { let _ = h.join(); }
        }
        let mut out: Vec<rh::RepoStatus> = roots.iter().map(|r| rh::status(r)).collect();
        // Then everything else on GitHub, so cloud tasks can target any repo.
        if let Some(token) = github_token(&engine) {
            if let Ok(remote) = github_repos(&token, fetch.unwrap_or(false)) {
                for r in remote {
                    if !out.iter().any(|l| l.github.as_deref() == Some(r.html_url.as_str())) {
                        out.push(rh::remote_status(&r));
                    }
                }
            }
        }
        Ok(out)
    }).await
}

// ---------------- GitHub ----------------

use aiuc_core::github;

fn github_token(engine: &Mutex<Engine>) -> Option<String> {
    engine.lock().ok()?.store.kv_get(github::KV_TOKEN).ok().flatten().filter(|t| !t.is_empty())
}

/// The user's GitHub repos, cached for ten minutes.
fn github_repos(token: &str, refresh: bool) -> anyhow::Result<Vec<github::GhRepo>> {
    type Cached = Option<(Instant, String, Vec<github::GhRepo>)>;
    static CACHE: Mutex<Cached> = Mutex::new(None);
    if !refresh {
        if let Some((at, t, repos)) = CACHE.lock().ok().and_then(|c| c.clone()) {
            if t == token && at.elapsed() < Duration::from_secs(600) { return Ok(repos); }
        }
    }
    let repos = github::repos(token)?;
    if let Ok(mut c) = CACHE.lock() { *c = Some((Instant::now(), token.to_owned(), repos.clone())); }
    Ok(repos)
}

#[derive(Serialize)]
struct GithubStatus {
    login: Option<String>,
    /// The GitHub CLI is installed and signed in, so connecting takes one click.
    cli: bool,
    /// This build can "Sign in with GitHub".
    device_flow: bool,
}

#[tauri::command]
async fn github_status(state: State<'_, AppState>) -> CmdResult<GithubStatus> {
    let engine = state.engine.clone();
    blocking(move || {
        let login = engine.lock().map_err(err)?.store.kv_get(github::KV_LOGIN).map_err(err)?;
        let connected = github_token(&engine).is_some();
        Ok(GithubStatus { login: login.filter(|_| connected), cli: !connected && github::gh_cli_token().is_some(), device_flow: github::client_id().is_some() })
    }).await
}

fn save_github(engine: &Mutex<Engine>, token: &str) -> CmdResult<String> {
    let login = github::whoami(token).map_err(err)?;
    let eng = engine.lock().map_err(err)?;
    eng.store.kv_set(github::KV_TOKEN, token).map_err(err)?;
    eng.store.kv_set(github::KV_LOGIN, &login).map_err(err)?;
    Ok(login)
}

/// Connect with the GitHub CLI's login, or a pasted token.
#[tauri::command]
async fn github_connect(state: State<'_, AppState>, token: Option<String>) -> CmdResult<String> {
    let engine = state.engine.clone();
    blocking(move || {
        let token = match token.map(|t| t.trim().to_owned()).filter(|t| !t.is_empty()) {
            Some(t) => t,
            None => github::gh_cli_token().ok_or("The GitHub CLI isn't signed in. Run `gh auth login`, or paste a token.")?,
        };
        save_github(&engine, &token)
    }).await
}

/// "Sign in with GitHub": returns the code to enter, and opens the page.
#[tauri::command]
async fn github_device_start(app: AppHandle) -> CmdResult<github::DeviceCode> {
    let id = github::client_id().ok_or("Sign in with GitHub isn't set up in this build.")?;
    let code = blocking(move || github::device_start(id).map_err(err)).await?;
    let _ = app.opener().open_url(code.verification_uri.clone(), None::<&str>);
    Ok(code)
}

/// Poll once; returns the login when approved, nothing while waiting.
#[tauri::command]
async fn github_device_poll(state: State<'_, AppState>, device_code: String) -> CmdResult<Option<String>> {
    let id = github::client_id().ok_or("Sign in with GitHub isn't set up in this build.")?;
    let engine = state.engine.clone();
    blocking(move || match github::device_poll(id, &device_code).map_err(err)? {
        Some(token) => save_github(&engine, &token).map(Some),
        None => Ok(None),
    }).await
}

#[tauri::command]
fn github_disconnect(state: State<AppState>) -> CmdResult<()> {
    let eng = state.engine.lock().map_err(err)?;
    eng.store.kv_del(github::KV_TOKEN).map_err(err)?;
    eng.store.kv_del(github::KV_LOGIN).map_err(err)
}

/// Clone a GitHub repo into a folder you pick, and add it to the list.
#[tauri::command]
async fn clone_repo(app: AppHandle, state: State<'_, AppState>, full_name: String) -> CmdResult<Option<String>> {
    use tauri_plugin_dialog::DialogExt;
    let (tx, rx) = std::sync::mpsc::channel();
    app.dialog().file().set_title(format!("Where should {full_name} go?")).pick_folder(move |f| { let _ = tx.send(f); });
    let engine = state.engine.clone();
    blocking(move || {
        let Some(Some(folder)) = rx.recv().ok() else { return Ok(None) };
        let parent = folder.into_path().map_err(err)?;
        if !full_name.split('/').all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_alphanumeric() || "-_.".contains(c))) {
            return Err("Not a repository name.".into());
        }
        let token = github_token(&engine);
        let root = rh::clone(&parent, &full_name, token.as_deref()).map_err(err)?;
        let mut eng = engine.lock().map_err(err)?;
        eng.settings.repos.retain(|r| r != &root);
        eng.settings.repos.insert(0, root.clone());
        eng.save_settings().map_err(err)?;
        Ok(Some(root.to_string_lossy().into_owned()))
    }).await
}

#[tauri::command]
async fn setup_repo(path: PathBuf, commit: bool) -> CmdResult<rh::SetupResult> {
    blocking(move || rh::setup(&path, commit).map_err(err)).await
}

#[tauri::command]
async fn add_repo(app: AppHandle, state: State<'_, AppState>) -> CmdResult<Option<PathBuf>> {
    use tauri_plugin_dialog::DialogExt;
    let (tx, rx) = std::sync::mpsc::channel();
    app.dialog().file().set_title("Choose a project folder").pick_folder(move |f| { let _ = tx.send(f); });
    let engine = state.engine.clone();
    blocking(move || {
        let Some(Some(folder)) = rx.recv().ok() else { return Ok(None) };
        let folder = folder.into_path().map_err(err)?;
        let root = rh::repo_root(&folder).map_err(err)?;
        let mut eng = engine.lock().map_err(err)?;
        eng.settings.repos.retain(|r| r != &root);
        eng.settings.repos.insert(0, root.clone());
        eng.save_settings().map_err(err)?;
        Ok(Some(root))
    }).await
}

#[tauri::command]
fn forget_repo(state: State<AppState>, path: PathBuf) -> CmdResult<()> {
    let mut eng = state.engine.lock().map_err(err)?;
    eng.settings.repos.retain(|r| r != &path);
    eng.save_settings().map_err(err)
}

#[tauri::command]
async fn preview_repo_handoff(window: tauri::WebviewWindow, state: State<'_, AppState>, path: PathBuf, target: Tool) -> CmdResult<Preview> {
    if window.label() != "widget" { return Err("Open the widget to preview a handoff.".into()); }
    let engine = state.engine.clone();
    let pending = state.handoff.clone();
    blocking(move || {
        // Only repositories the widget lists, so a page can't point this at any folder.
        let root = rh::repo_root(&path).map_err(err)?;
        if !repo_candidates(&engine)?.contains(&root) { return Err("That repository isn't in your list.".into()); }
        let status = rh::status(&root);
        let latest = status.latest.as_ref().ok_or("This repo has no handoff yet. It appears after the first AI session that runs there.")?;
        let cloud = !handoff::destinations(target, true).iter().any(|d| d.dest != handoff::Dest::Web);
        let prompt = rh::continue_prompt(&status.name, latest, cloud);
        let place = if latest.branch.is_empty() { status.name.clone() } else { format!("{} · {}", status.name, latest.branch) };
        new_preview(&pending, Some(root), target, prompt, place)
    }).await
}


#[tauri::command]
fn get_settings(state: State<AppState>) -> CmdResult<Settings> {
    Ok(state.engine.lock().map_err(err)?.settings.clone())
}

fn apply_autostart(app: &AppHandle, settings: &Settings) -> CmdResult<()> {
    if !settings.onboarded { return Ok(()); }
    let manager = app.autolaunch();
    if manager.is_enabled().map_err(err)? != settings.launch_at_login {
        if settings.launch_at_login { manager.enable() } else { manager.disable() }.map_err(err)?;
    }
    Ok(())
}

#[tauri::command]
fn save_settings(app: AppHandle, state: State<AppState>, mut settings: Settings) -> CmdResult<()> {
    apply_autostart(&app, &settings)?;
    let old_hotkey = {
        let mut eng = state.engine.lock().map_err(err)?;
        let old = eng.settings.hotkey.clone();
        // Sign-in can complete while onboarding or Settings is still open.
        settings.claude_web = eng.settings.claude_web;
        // Repos are managed from the Handoff tab, not the settings form.
        settings.repos = eng.settings.repos.clone();
        // Agent status installs hooks, so it changes through its own command.
        settings.agent_status = eng.settings.agent_status;
        let approvals_changed = settings.approvals != eng.settings.approvals;
        eng.settings = settings.clone();
        eng.save_settings().map_err(err)?;
        if approvals_changed && settings.agent_status {
            if let Ok(exe) = std::env::current_exe() { let _ = aiuc_core::agent_status::install(&exe, settings.approvals); }
        }
        old
    };
    if old_hotkey != settings.hotkey {
        let gs = app.global_shortcut();
        let _ = gs.unregister(old_hotkey.as_str());
        gs.register(settings.hotkey.as_str()).map_err(|e| format!("Couldn't use {}: {e}", settings.hotkey))?;
    }
    Ok(())
}

#[tauri::command]
fn get_plans(state: State<AppState>) -> CmdResult<BTreeMap<Tool, Vec<Plan>>> {
    let eng = state.engine.lock().map_err(err)?;
    Ok(Tool::ALL.iter().map(|t| (*t, eng.plans.plans(*t).to_vec())).collect())
}

#[derive(Serialize)]
struct Detected {
    tool: Tool,
    label: &'static str,
    found: bool,
    roots: Vec<String>,
}

#[tauri::command]
fn detect_tools(state: State<AppState>) -> CmdResult<Vec<Detected>> {
    let eng = state.engine.lock().map_err(err)?;
    Ok(Tool::ALL
        .iter()
        .map(|t| {
            let roots = aiuc_core::paths::detect(*t, &eng.settings.tool(*t).extra_roots);
            Detected {
                tool: *t,
                label: t.label(),
                found: !roots.is_empty(),
                roots: roots.iter().map(|p| p.display().to_string()).collect(),
            }
        })
        .collect())
}

/// The UI reports the shape that should take the mouse (pill or panel).
#[tauri::command]
fn set_hit_rect(state: State<AppState>, rect: window::Rect) -> CmdResult<()> {
    *state.hit.lock().map_err(err)? = rect;
    Ok(())
}

/// Give the widget keyboard focus (for typing in Settings).
#[tauri::command]
fn focus_widget(app: AppHandle) -> CmdResult<()> {
    window::widget(&app).ok_or("no widget window")?.set_focus().map_err(err)
}

// The Claude commands are async: on Windows, creating a webview or touching its
// cookies from a synchronous command deadlocks (WebView2, wry#583).
#[tauri::command]
async fn connect_claude(app: AppHandle, state: State<'_, AppState>) -> CmdResult<()> {
    claude_web::open_login(&app, &state.web).map_err(err)
}

#[tauri::command]
async fn claude_session_key(app: AppHandle, state: State<'_, AppState>, key: String) -> CmdResult<()> {
    claude_web::use_session_key(&app, &state.web, &key)?;
    let mut eng = state.engine.lock().map_err(err)?;
    eng.settings.claude_web = true;
    eng.save_settings().map_err(err)
}

#[tauri::command]
async fn disconnect_claude(app: AppHandle, state: State<'_, AppState>) -> CmdResult<()> {
    claude_web::disconnect(&app);
    let mut eng = state.engine.lock().map_err(err)?;
    eng.store.kv_set("claude_web_expired", "0").map_err(err)?;
    eng.settings.claude_web = false;
    eng.live_errors.remove(&Tool::ClaudeCode);
    eng.save_settings().map_err(err)
}

/// Re-read logs and gauges now (the "Check setup" screen's refresh button).
#[tauri::command]
fn refresh_now(state: State<AppState>) -> CmdResult<()> {
    state.web.poll_now.store(true, std::sync::atomic::Ordering::SeqCst);
    let mut eng = state.engine.lock().map_err(err)?;
    eng.scan().map_err(err)?;
    Ok(())
}

#[derive(Serialize)]
struct Diagnostics {
    app_version: &'static str,
    platform: &'static str,
    data_dir: String,
    claude_web: bool,
    claude_code_login: bool,
    signing_in: bool,
    tools: Vec<aiuc_core::snapshot::ToolDiagnostics>,
}

#[tauri::command]
fn diagnostics(state: State<AppState>) -> CmdResult<Diagnostics> {
    let eng = state.engine.lock().map_err(err)?;
    Ok(Diagnostics {
        app_version: env!("CARGO_PKG_VERSION"),
        platform: window::platform(),
        data_dir: aiuc_core::paths::app_data_dir().display().to_string(),
        claude_web: eng.settings.claude_web,
        // Cached by the worker: on a Mac each check reads the Keychain.
        claude_code_login: CLAUDE_CODE_LOGIN.load(std::sync::atomic::Ordering::Relaxed),
        signing_in: state.web.signing_in.load(std::sync::atomic::Ordering::SeqCst),
        tools: eng.diagnostics(Utc::now()),
    })
}

#[tauri::command]
fn layout() -> window::Layout {
    window::layout()
}

#[tauri::command]
fn platform() -> &'static str {
    window::platform()
}

#[tauri::command]
fn dismiss_alert(state: State<AppState>, id: String) -> CmdResult<()> {
    state.engine.lock().map_err(err)?.dismiss(&id).map_err(err)
}

/// The one button on an alert. "Stop" interrupts the session; "Switch" is advice
/// only in v1 (the user decides), so it dismisses with a pointer to the other tool.
/// Takes only the id: the alert itself comes from the engine's own snapshot, so the
/// webview can never choose which process gets interrupted.
#[tauri::command]
fn alert_action(state: State<AppState>, id: String) -> CmdResult<String> {
    let alert = state
        .last
        .lock()
        .map_err(err)?
        .as_ref()
        .and_then(|s| s.alerts.iter().find(|a| a.id == id).cloned())
        .ok_or("That alert has already cleared.")?;
    let eng = state.engine.lock().map_err(err)?;
    match alert.action.kind.as_str() {
        "stop" => {
            let n = eng.stop_runaway(&alert).map_err(err)?;
            Ok(format!("Stopped {} session{}.", alert.tool.label(), if n == 1 { "" } else { "s" }))
        }
        "switch" => {
            eng.dismiss(&alert.id).map_err(err)?;
            let target = alert.action.target_tool.map(|t| t.label()).unwrap_or("another tool");
            Ok(format!("{target} has room. Open it in your terminal to keep going."))
        }
        _ => {
            eng.dismiss(&alert.id).map_err(err)?;
            Ok("ok".into())
        }
    }
}

#[tauri::command]
fn accept_nudge(state: State<AppState>) -> CmdResult<()> {
    let s = snapshot_now(&state)?;
    if let Some(n) = s.nudge {
        state.engine.lock().map_err(err)?.accept_nudge(&n).map_err(err)?;
    }
    Ok(())
}

#[tauri::command]
fn accept_audit(state: State<AppState>, tool: Tool) -> CmdResult<()> {
    state.engine.lock().map_err(err)?.accept_audit(tool).map_err(err)
}

#[tauri::command]
fn start_trial(state: State<AppState>) -> CmdResult<()> {
    let eng = state.engine.lock().map_err(err)?;
    aiuc_core::entitlements::start_trial(&eng.store, Utc::now()).map_err(err)
}

#[tauri::command]
fn activate_licence(state: State<AppState>, key: String) -> CmdResult<String> {
    let host = sysinfo::System::host_name().unwrap_or_else(|| "device".into());
    let eng = state.engine.lock().map_err(err)?;
    let rec = aiuc_core::licence::activate(&eng.store, key.trim(), &host).map_err(err)?;
    Ok(format!("{:?}", rec.tier).to_lowercase())
}

#[tauri::command]
fn deactivate_licence(state: State<AppState>) -> CmdResult<()> {
    let eng = state.engine.lock().map_err(err)?;
    aiuc_core::licence::deactivate(&eng.store).map_err(err)
}

#[tauri::command]
fn open_url(app: AppHandle, url: String) -> CmdResult<()> {
    if !(url.starts_with("https://") || url.starts_with("http://localhost")) {
        return Err("only https links".into());
    }
    app.opener().open_url(url, None::<&str>).map_err(err)
}

#[tauri::command]
fn join_sync(state: State<AppState>, server: String, code: String, email: String) -> CmdResult<()> {
    let host = sysinfo::System::host_name().unwrap_or_else(|| "device".into());
    let sync = aiuc_core::sync::join(&server, &code, &email, &host).map_err(err)?;
    let mut eng = state.engine.lock().map_err(err)?;
    eng.settings.sync = sync;
    eng.save_settings().map_err(err)
}

#[tauri::command]
fn quit(app: AppHandle) {
    app.exit(0);
}

// ---------------- background worker ----------------

fn notify(app: &AppHandle, title: &str, body: &str) {
    let _ = app.notification().builder().title(title).body(body).show();
}

fn fetch_tables(api_base: &str) -> Option<(String, String)> {
    let agent: ureq::Agent = ureq::Agent::config_builder().timeout_global(Some(Duration::from_secs(10))).build().into();
    let get = |p: &str| agent.get(&format!("{}{}", api_base.trim_end_matches('/'), p)).call().ok()?.body_mut().read_to_string().ok();
    Some((get("/v1/prices")?, get("/v1/plans")?))
}

fn update_tray(app: &AppHandle, snap: &Snapshot, tray_state: &mut Option<(String, [u8; 3])>) {
    let tight = snap.tools.iter().filter(|t| t.enabled && !t.locked)
        .filter_map(|t| t.headroom_pct.map(|h| (t, h))).min_by(|a,b| a.1.total_cmp(&b.1));
    let (tip, rgb) = match tight {
        Some((t, h)) => {
            let rgb = match t.color.as_str() { "red" => [244, 75, 79], "amber" => [245, 174, 57], _ => [62, 199, 119] };
            let note = t.live_note.as_ref().map(|s| format!(" ({s})")).unwrap_or_default();
            (format!("{}: {:.0}% used{note}", t.label, 100.0 - h), rgb)
        }
        None => ("AI Usage Controller".to_owned(), [150, 150, 150]),
    };
    if tray_state.as_ref() != Some(&(tip.clone(), rgb)) {
        if let Some(tray) = app.tray_by_id("main") {
            let mut rgba = Vec::with_capacity(16 * 16 * 4);
            for y in 0..16i32 { for x in 0..16i32 {
                rgba.extend(rgb);
                rgba.push(if (2*x-15).pow(2) + (2*y-15).pow(2) <= 196 { 255 } else { 0 });
            }}
            let _ = tray.set_icon(Some(tauri::image::Image::new_owned(rgba, 16, 16)));
            let _ = tray.set_tooltip(Some(&tip));
            *tray_state = Some((tip, rgb));
        }
    }
}

fn spawn_worker(app: AppHandle, engine: Arc<Mutex<Engine>>, last: Arc<Mutex<Option<Snapshot>>>) {
    std::thread::spawn(move || {
        let mut tray_state = None;
        let mut ticker = agents::Ticker::default();
        agents::refresh_hooks(&engine);
        let mut notified: HashSet<String> = HashSet::new();
        let mut last_live = Instant::now() - LIVE_EVERY;
        let mut last_sync = Instant::now() - SYNC_EVERY;
        let mut last_tables = Instant::now() - TABLES_EVERY;
        loop {
            let started = Instant::now();
            let (live_on, sync, api_base) = match engine.lock() {
                Ok(mut eng) => {
                    let _ = eng.scan();
                    // A connected claude.ai account (claude_web.rs) reports the same
                    // plan limits, so the Claude Code login is only the fallback.
                    let use_oauth = eng.settings.live_gauges
                        && !eng.settings.claude_web
                        && eng.settings.tool(Tool::ClaudeCode).enabled
                        && eng.settings.tool(Tool::ClaudeCode).plan_id.as_deref() != Some("api");
                    (use_oauth, eng.settings.sync.clone(), eng.settings.api_base.clone())
                }
                Err(_) => break,
            };

            // Network work happens without holding the engine lock.
            if live_on && last_live.elapsed() >= LIVE_EVERY {
                last_live = Instant::now();
                let has_login = aiuc_core::live::has_claude_code_login();
                CLAUDE_CODE_LOGIN.store(has_login, std::sync::atomic::Ordering::Relaxed);
                // No Claude Code login is normal for claude.ai users: say nothing.
                if has_login {
                    let res = aiuc_core::live::fetch_claude_gauges(Utc::now()).map_err(|e| e.to_string());
                    if let Ok(mut eng) = engine.lock() {
                        eng.ingest_live(Tool::ClaudeCode, res);
                    }
                }
            }
            if last_tables.elapsed() >= TABLES_EVERY {
                last_tables = Instant::now();
                if let Some((prices, plans)) = fetch_tables(&api_base) {
                    if let Ok(mut eng) = engine.lock() {
                        let _ = eng.update_tables(Some(&prices), Some(&plans));
                    }
                }
                if let Ok(eng) = engine.lock() {
                    let _ = aiuc_core::licence::validate(&eng.store);
                }
            }
            if sync.enabled && last_sync.elapsed() >= SYNC_EVERY {
                last_sync = Instant::now();
                let audit = last.lock().ok().and_then(|l| l.as_ref().map(|s| s.audit.clone())).unwrap_or_default();
                if let Ok(mut eng) = engine.lock() {
                    let plans: BTreeMap<Tool, Option<String>> = Tool::ALL.iter().map(|t| (*t, eng.settings.tool(*t).plan_id)).collect();
                    let meta = serde_json::json!({
                        "plans": plans,
                        "audit": audit.iter().map(|a| serde_json::json!({
                            "tool": a.tool, "action": a.action, "current_plan": a.current_plan,
                            "monthly_saving_usd": a.monthly_saving_usd, "peak_used_pct": a.peak_used_pct
                        })).collect::<Vec<_>>(),
                        "app_version": env!("CARGO_PKG_VERSION"),
                    });
                    let _ = aiuc_core::sync::push(&mut eng.store, &sync, &meta);
                }
                if let Ok(summary) = aiuc_core::sync::pull_summary(&sync) {
                    if let Ok(mut eng) = engine.lock() {
                        eng.remote = Some(summary);
                    }
                }
            }

            let snap = engine.lock().ok().and_then(|mut e| {
                let notifications = e.settings.alerts.notifications;
                e.snapshot(Utc::now()).ok().map(|s| (s, notifications))
            });
            if let Some((snap, notifications)) = snap {
                update_tray(&app, &snap, &mut tray_state);
                for a in &snap.alerts {
                    if notified.insert(a.id.clone()) && notifications {
                        let title = if a.kind == "runaway" { "Possible runaway loop" } else { "AI runway running low" };
                        notify(&app, title, &a.message);
                    }
                }
                ticker.poll();
                ticker.tick(&app, &engine, &snap);
                watch::tick(&app, &engine, &snap);
                let _ = app.emit("snapshot", &snap);
                if let Ok(mut l) = last.lock() {
                    *l = Some(snap);
                }
            }
            let spent = started.elapsed();
            if spent < TICK {
                std::thread::sleep(TICK - spent);
            }
        }
    });
}

#[cfg(feature = "updater")]
fn spawn_updater(app: AppHandle) {
    use tauri_plugin_updater::UpdaterExt;
    tauri::async_runtime::spawn(async move {
        loop {
            if let Ok(updater) = app.updater() {
                if let Ok(Some(update)) = updater.check().await {
                    let _ = update.download_and_install(|_, _| {}, || {}).await;
                }
            }
            tokio_sleep(CDuration::hours(12)).await;
        }
    });
}

#[cfg(feature = "updater")]
async fn tokio_sleep(d: CDuration) {
    let _ = tauri::async_runtime::spawn_blocking(move || std::thread::sleep(d.to_std().unwrap_or(Duration::from_secs(3600)))).await;
}

#[allow(dead_code)]
fn _keep(_: CDuration) {}

// ---------------- app ----------------

pub use agents::hook_main;

pub fn run() {
    let engine = Engine::open_default().expect("open usage database");
    let hotkey = engine.settings.hotkey.clone();
    let engine = Arc::new(Mutex::new(engine));
    let last = Arc::new(Mutex::new(None));
    let hit = Arc::new(Mutex::new(window::Rect::default()));
    let web = claude_web::ClaudeWeb::new();

    let mut builder = tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _, _| {
            let _ = app.emit("toggle", ());
        }))
        .plugin(tauri_plugin_autostart::init(tauri_plugin_autostart::MacosLauncher::LaunchAgent, None))
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(|app, shortcut, event| {
                    // Ask: tap to open (it looks at your screen first), hold to talk.
                    let ask_key = ask::ASK_HOTKEY.parse::<tauri_plugin_global_shortcut::Shortcut>().ok();
                    if ask_key.is_some_and(|k| k.id() == shortcut.id()) {
                        match event.state() {
                            ShortcutState::Pressed => buddy::on_press(app),
                            ShortcutState::Released => buddy::on_release(app),
                        }
                        return;
                    }
                    if event.state() == ShortcutState::Pressed {
                        if let Some(w) = window::widget(app) {
                            let _ = w.show();
                            let _ = w.set_focus();
                        }
                        // The dispatcher's hotkey opens straight into the task box.
                        let dispatch = DISPATCH_HOTKEY.parse::<tauri_plugin_global_shortcut::Shortcut>().ok();
                        if dispatch.is_some_and(|d| d.id() == shortcut.id()) {
                            let _ = app.emit("open-view", "agents");
                        } else {
                            let _ = app.emit("toggle", ());
                        }
                    }
                })
                .build(),
        );
    #[cfg(feature = "updater")]
    {
        builder = builder.plugin(tauri_plugin_updater::Builder::new().build());
    }

    builder = builder
        .manage(AppState { handoff: Default::default(), engine: engine.clone(), last: last.clone(), hit: hit.clone(), web: web.clone() })
        .manage(web.clone())
        .invoke_handler(tauri::generate_handler![
            get_snapshot,
            preview_handoff,
            test_handoff_offers,
            launch_handoff,
            list_repos,
            setup_repo,
            add_repo,
            forget_repo,
            preview_repo_handoff,
            github_status,
            github_connect,
            github_device_start,
            github_device_poll,
            github_disconnect,
            clone_repo,
            dispatch::plan_dispatch,
            dispatch::run_dispatch,
            dispatch::dismiss_agent,
            dispatch::start_dictation,
            dispatch::run_queued,
            agents::set_agent_status,
            ask::ask_context,
            ask::ask_forget,
            ask::ask_start,
            ask::ask_cancel,
            ask::ask_paste_back,
            ask::ask_copy,
            ask::ask_run,
            ask::ask_open_in,
            buddy::buddy_state,
            buddy::buddy_hide,
            buddy::last_voice_turn,
            ask::ask_hide_points,
            ask::polish_prompt,
            agents::answer_approval,
            agents::verify_task,
            agents::verify_output,
            get_settings,
            save_settings,
            get_plans,
            detect_tools,
            set_hit_rect,
            focus_widget,
            connect_claude,
            claude_session_key,
            disconnect_claude,
            refresh_now,
            diagnostics,
            layout,
            platform,
            dismiss_alert,
            dismiss_daily_recap,
            watch::add_watch,
            watch::list_watches,
            watch::remove_watch,
            alert_action,
            accept_nudge,
            accept_audit,
            start_trial,
            activate_licence,
            deactivate_licence,
            open_url,
            join_sync,
            quit
        ])
        .setup(move |app| {
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);

            if let Some(w) = window::widget(app) {
                window::mac_float(&w);
                window::place(&w)?;
                w.show()?;
                // Click-through is switched on by hover.rs once the window is up
                // (on Linux it must be realised first).
            }

            if let Err(e) = app.global_shortcut().register(hotkey.as_str()) {
                eprintln!("hotkey {hotkey} unavailable: {e}");
            }
            if let Err(e) = app.global_shortcut().register(ask::ASK_HOTKEY) {
                eprintln!("hotkey {} unavailable: {e}", ask::ASK_HOTKEY);
            }
            capture::spawn_clipboard_clock();
            if let Err(e) = app.global_shortcut().register(DISPATCH_HOTKEY) {
                eprintln!("hotkey {DISPATCH_HOTKEY} unavailable: {e}");
            }

            if let Ok(eng) = engine.lock() {
                if let Err(e) = apply_autostart(app.handle(), &eng.settings) {
                    eprintln!("Could not update launch at login: {e}");
                }
            }
            let open = MenuItem::with_id(app, "open", "Open panel", true, None::<&str>)?;
            let settings = MenuItem::with_id(app, "settings", "Settings…", true, None::<&str>)?;
            let hide = MenuItem::with_id(app, "hide", "Hide widget", true, None::<&str>)?;
            let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&open, &settings, &hide, &PredefinedMenuItem::separator(app)?, &quit])?;
            let mut tray = TrayIconBuilder::with_id("main")
                .tooltip("AI Usage Controller")
                .menu(&menu)
                .show_menu_on_left_click(false)
                .on_tray_icon_event(|tray, event| {
                    use tauri::tray::{MouseButton, MouseButtonState, TrayIconEvent};
                    if matches!(event, TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. }) {
                        let app = tray.app_handle();
                        if let Some(w) = window::widget(app) { let _ = w.show(); let _ = w.set_focus(); }
                        let _ = app.emit("toggle", ());
                    }
                })
                .on_menu_event(|app, e| match e.id.as_ref() {
                    "open" => {
                        if let Some(w) = window::widget(app) {
                            let _ = w.show();
                            let _ = w.set_focus();
                        }
                        let _ = app.emit("open-view", "panel");
                    }
                    "settings" => {
                        if let Some(w) = window::widget(app) {
                            let _ = w.show();
                            let _ = w.set_focus();
                        }
                        let _ = app.emit("open-view", "settings");
                    }
                    "hide" => {
                        if let Some(w) = window::widget(app) {
                            if w.is_visible().unwrap_or(true) {
                                let _ = w.hide();
                            } else {
                                let _ = w.show();
                            }
                        }
                    }
                    "quit" => app.exit(0),
                    _ => {}
                });
            if let Some(icon) = app.default_window_icon() {
                tray = tray.icon(icon.clone());
            }
            tray.build(app)?;

            spawn_worker(app.handle().clone(), engine.clone(), last.clone());
            hover::spawn(app.handle().clone(), hit.clone());
            claude_web::spawn(app.handle().clone(), web.clone(), engine.clone());
            clipboard_watch::spawn(app.handle().clone(), engine.clone());
            #[cfg(feature = "updater")]
            spawn_updater(app.handle().clone());
            Ok(())
        });

    builder
        .run(tauri::generate_context!())
        .expect("error while running AI Usage Controller");
}
