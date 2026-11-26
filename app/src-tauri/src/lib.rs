mod claude_web;
mod hover;
mod window;

use aiuc_core::model::Tool;
use aiuc_core::plans::Plan;
use aiuc_core::settings::Settings;
use aiuc_core::{Engine, Snapshot};
use chrono::{Duration as CDuration, Utc};
use serde::Serialize;
use std::collections::{BTreeMap, HashSet};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Emitter, State};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};
use tauri_plugin_notification::NotificationExt;
use tauri_plugin_opener::OpenerExt;

const TICK: Duration = Duration::from_secs(3);
const LIVE_EVERY: Duration = Duration::from_secs(60);
const SYNC_EVERY: Duration = Duration::from_secs(60);
const TABLES_EVERY: Duration = Duration::from_secs(6 * 3600);
static CLAUDE_CODE_LOGIN: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

struct AppState {
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
fn get_settings(state: State<AppState>) -> CmdResult<Settings> {
    Ok(state.engine.lock().map_err(err)?.settings.clone())
}

#[tauri::command]
fn save_settings(app: AppHandle, state: State<AppState>, settings: Settings) -> CmdResult<()> {
    let old_hotkey = {
        let mut eng = state.engine.lock().map_err(err)?;
        let old = eng.settings.hotkey.clone();
        eng.settings = settings.clone();
        eng.save_settings().map_err(err)?;
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

fn spawn_worker(app: AppHandle, engine: Arc<Mutex<Engine>>, last: Arc<Mutex<Option<Snapshot>>>) {
    std::thread::spawn(move || {
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
                for a in &snap.alerts {
                    if notified.insert(a.id.clone()) && notifications {
                        let title = if a.kind == "runaway" { "Possible runaway loop" } else { "AI runway running low" };
                        notify(&app, title, &a.message);
                    }
                }
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
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(|app, _shortcut, event| {
                    if event.state() == ShortcutState::Pressed {
                        if let Some(w) = window::widget(app) {
                            let _ = w.set_focus();
                        }
                        let _ = app.emit("toggle", ());
                    }
                })
                .build(),
        );
    #[cfg(feature = "updater")]
    {
        builder = builder.plugin(tauri_plugin_updater::Builder::new().build());
    }

    builder = builder
        .manage(AppState { engine: engine.clone(), last: last.clone(), hit: hit.clone(), web: web.clone() })
        .manage(web.clone())
        .invoke_handler(tauri::generate_handler![
            get_snapshot,
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

            let open = MenuItem::with_id(app, "open", "Open panel", true, None::<&str>)?;
            let settings = MenuItem::with_id(app, "settings", "Settings…", true, None::<&str>)?;
            let hide = MenuItem::with_id(app, "hide", "Hide widget", true, None::<&str>)?;
            let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&open, &settings, &hide, &PredefinedMenuItem::separator(app)?, &quit])?;
            let mut tray = TrayIconBuilder::with_id("main")
                .tooltip("AI Usage Controller")
                .menu(&menu)
                .show_menu_on_left_click(true)
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
            #[cfg(feature = "updater")]
            spawn_updater(app.handle().clone());
            Ok(())
        });

    builder
        .run(tauri::generate_context!())
        .expect("error while running AI Usage Controller");
}
