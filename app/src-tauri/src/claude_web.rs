//! "Connect Claude account": read the same plan limits claude.ai shows in
//! Settings → Usage, for people who use claude.ai or the Claude desktop app
//! (which leave no logs on disk).
//!
//! How: the user signs in to claude.ai in a small window inside this app. That
//! window is then hidden and kept parked on a tiny claude.ai JSON page; once a
//! minute it runs `fetch()` against claude.ai's own usage endpoint with its own
//! cookies, and we read the result back. The login never leaves that webview:
//! the app doesn't copy the cookie, and requests only ever go to claude.ai.

use aiuc_core::model::Tool;
use aiuc_core::Engine;
use chrono::Utc;
use serde_json::Value;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tauri::webview::cookie::Cookie;
use tauri::{AppHandle, Emitter, Manager, Url, WebviewUrl, WebviewWindow, WebviewWindowBuilder};

pub const LABEL: &str = "claude";
const LOGIN_URL: &str = "https://claude.ai/login";
/// Small same-origin page to park on, so the hidden webview stays light.
const PARK_URL: &str = "https://claude.ai/api/organizations";
const EVERY: Duration = Duration::from_secs(60);

/// macOS's web view leaves "Safari" out of its user agent, and Google sign-in
/// then refuses it, so add the same suffix Safari sends. Elsewhere the engine's
/// own user agent is kept: on Windows it is already Microsoft Edge's, and a
/// made-up one would not match the engine and trips claude.ai's bot check.
#[cfg(target_os = "macos")]
const UA: Option<&str> = Some("Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/18.5 Safari/605.1.15");
#[cfg(not(target_os = "macos"))]
const UA: Option<&str> = None;

/// Kicks off a fetch; the result lands in `window.__aiuc` a moment later.
const KICK_JS: &str = r#"(function(){if(location.origin!=='https://claude.ai'||window.__aiucBusy)return;window.__aiucBusy=1;(async function(){try{
var r=await fetch('/api/organizations',{credentials:'include',signal:AbortSignal.timeout(15000),headers:{'accept':'application/json'}});
if(new URL(r.url).pathname.startsWith('/login')||r.status===401||r.status===403)throw new Error('signed_out');
if(!r.ok)throw new Error('organizations HTTP '+r.status);
var orgs=await r.json();
var org=(orgs.find(function(o){return (o.capabilities||[]).indexOf('chat')>=0})||orgs[0]);
if(!org)throw new Error('no Claude organization on this account');
var u=await fetch('/api/organizations/'+org.uuid+'/usage',{credentials:'include',signal:AbortSignal.timeout(15000),headers:{'accept':'application/json'}});
if(new URL(u.url).pathname.startsWith('/login')||u.status===401||u.status===403)throw new Error('signed_out');
if(!u.ok)throw new Error('usage HTTP '+u.status);
window.__aiuc={ok:true,data:await u.json()};
}catch(e){window.__aiuc={ok:false,error:String(e&&e.message||e)}}finally{window.__aiucBusy=0}})()})()"#;

const READ_JS: &str = "JSON.stringify(window.__aiuc||null)";

pub struct ClaudeWeb {
    /// Set to poll right away (just connected, or the user pressed refresh).
    pub poll_now: AtomicBool,
    /// The login window is open and we're waiting for the user to sign in.
    pub signing_in: AtomicBool,
}

impl ClaudeWeb {
    pub fn new() -> Arc<ClaudeWeb> {
        Arc::new(ClaudeWeb { poll_now: AtomicBool::new(true), signing_in: AtomicBool::new(false) })
    }
}

fn builder<'a>(app: &'a AppHandle, url: &str, visible: bool) -> tauri::Result<WebviewWindowBuilder<'a, tauri::Wry, AppHandle>> {
    let url = Url::parse(url).expect("constant URL");
    let mut b = WebviewWindowBuilder::new(app, LABEL, WebviewUrl::External(url))
        .title("Connect your Claude account")
        .inner_size(460.0, 700.0)
        .center()
        .visible(visible)
        .skip_taskbar(!visible)
        // claude.ai's Google sign-in may open a popup.
        .on_new_window(|_, _| tauri::webview::NewWindowResponse::Allow);
    if let Some(ua) = UA {
        b = b.user_agent(ua);
    }
    Ok(b)
}

fn window(app: &AppHandle) -> Option<WebviewWindow> {
    app.get_webview_window(LABEL)
}

/// Open the sign-in window (or bring it forward).
pub fn open_login(app: &AppHandle, state: &ClaudeWeb) -> tauri::Result<()> {
    state.signing_in.store(true, Ordering::SeqCst);
    if let Some(w) = window(app) {
        w.navigate(LOGIN_URL.parse().unwrap())?;
        w.show()?;
        w.set_focus()?;
        return Ok(());
    }
    let w = builder(app, LOGIN_URL, true)?.build()?;
    let app2 = app.clone();
    w.on_window_event(move |e| {
        // Closing the sign-in window just hides it; the webview keeps the login.
        if let tauri::WindowEvent::CloseRequested { api, .. } = e {
            api.prevent_close();
            if let Some(w) = window(&app2) {
                let _ = w.hide();
            }
            if let Some(s) = app2.try_state::<Arc<ClaudeWeb>>() {
                s.signing_in.store(false, Ordering::SeqCst);
            }
        }
    });
    Ok(())
}

/// Sign-in fallback: the user pastes the `sessionKey` cookie from their browser.
pub fn use_session_key(app: &AppHandle, state: &ClaudeWeb, key: &str) -> Result<(), String> {
    let key = key.trim().trim_start_matches("sessionKey=").to_string();
    if !key.starts_with("sk-ant-") || key.len() < 20 {
        return Err("That doesn't look like a claude.ai session key (it starts with sk-ant-).".into());
    }
    if window(app).is_none() {
        builder(app, PARK_URL, false).map_err(|e| e.to_string())?.build().map_err(|e| e.to_string())?;
    }
    let w = window(app).ok_or("couldn't open the Claude window")?;
    let cookie = Cookie::build(("sessionKey", key)).domain(".claude.ai").path("/").secure(true).http_only(true).build();
    w.set_cookie(cookie).map_err(|e| e.to_string())?;
    w.navigate(PARK_URL.parse().unwrap()).map_err(|e| e.to_string())?;
    let _ = w.hide();
    state.signing_in.store(false, Ordering::SeqCst);
    state.poll_now.store(true, Ordering::SeqCst);
    Ok(())
}

pub fn disconnect(app: &AppHandle) {
    if let Some(w) = window(app) {
        let _ = w.clear_all_browsing_data();
        let _ = w.destroy();
    }
}

fn has_session_cookie(w: &WebviewWindow) -> bool {
    w.cookies_for_url("https://claude.ai/".parse().unwrap())
        .map(|cs| cs.iter().any(|c| c.name() == "sessionKey" && !c.value().is_empty()))
        .unwrap_or(false)
}

fn parse_read(raw: &str) -> Option<Value> {
    // eval_with_callback JSON-encodes the expression's value, and our value is
    // itself a JSON string, so it may arrive encoded twice.
    let v: Value = serde_json::from_str(raw).ok()?;
    match v {
        Value::String(s) => serde_json::from_str(&s).ok(),
        other => Some(other),
    }
}

/// Background loop: finish sign-in when it happens, then poll once a minute.
pub fn spawn(app: AppHandle, state: Arc<ClaudeWeb>, engine: Arc<Mutex<Engine>>) {
    std::thread::spawn(move || {
        let mut last = Instant::now() - EVERY;
        loop {
            std::thread::sleep(Duration::from_millis(1500));
            let connected = engine.lock().map(|e| e.settings.claude_web).unwrap_or(false);

            // 1. Waiting for the user to finish signing in.
            if state.signing_in.load(Ordering::SeqCst) {
                let Some(w) = window(&app) else { continue };
                let on_app = w.url().map(|u| u.host_str() == Some("claude.ai") && !u.path().starts_with("/login") && !u.path().starts_with("/magic-link")).unwrap_or(false);
                if on_app && has_session_cookie(&w) {
                    state.signing_in.store(false, Ordering::SeqCst);
                    let _ = w.hide();
                    let _ = w.navigate(PARK_URL.parse().unwrap());
                    if let Ok(mut eng) = engine.lock() {
                        eng.settings.claude_web = true;
                        let _ = eng.save_settings();
                    }
                    let _ = app.emit("claude-connected", true);
                    state.poll_now.store(true, Ordering::SeqCst);
                    last = Instant::now() - EVERY;
                }
                continue;
            }

            // 2. Polling.
            if !connected {
                continue;
            }
            if !(state.poll_now.swap(false, Ordering::SeqCst) || last.elapsed() >= EVERY) {
                continue;
            }
            last = Instant::now();
            let w = match window(&app) {
                Some(w) => w,
                None => match builder(&app, PARK_URL, false).and_then(|b| b.build()) {
                    Ok(w) => {
                        // first load after a restart
                        std::thread::sleep(Duration::from_secs(4));
                        w
                    }
                    Err(_) => {
                        if let Ok(mut e) = engine.lock() { let _ = e.ingest_web(Err("Claude window unavailable".into())); }
                        continue;
                    }
                },
            };
            // If the parked page navigated somewhere odd, put it back.
            if w.url().map(|u| u.host_str() != Some("claude.ai")).unwrap_or(true) {
                let _ = w.navigate(PARK_URL.parse().unwrap());
                std::thread::sleep(Duration::from_secs(4));
            }
            let res = poll(&w);
            let send = engine.lock().ok().and_then(|mut e| e.ingest_web(res).ok()).unwrap_or(false);
            if send {
                super::notify(&app, "Claude signed out", "Open the widget and choose Reconnect.");
            }
        }
    });
}

fn poll(w: &WebviewWindow) -> Result<Vec<aiuc_core::model::GaugeSample>, String> {
    w.eval("window.__aiuc=null").map_err(|e| e.to_string())?;
    w.eval(KICK_JS).map_err(|e| e.to_string())?;
    let deadline = Instant::now() + Duration::from_secs(35);
    while Instant::now() < deadline {
        std::thread::sleep(Duration::from_secs(1));
        let (tx, rx) = std::sync::mpsc::channel();
        w.eval_with_callback(READ_JS, move |raw| { let _ = tx.send(parse_read(&raw)); })
            .map_err(|e| e.to_string())?;
        let Some(v) = rx.recv_timeout(Duration::from_secs(3)).ok().flatten() else { continue };
        if v.is_null() { continue; }
        if v["ok"] != true {
            return Err(v["error"].as_str().unwrap_or("Claude unavailable").to_owned());
        }
        let gauges = aiuc_core::live::parse_claude_usage_from(&v["data"], Utc::now(), "claude_web");
        return if gauges.is_empty() { Err("Claude returned no usage numbers".into()) } else { Ok(gauges) };
    }
    Err("Claude did not respond".into())
}
