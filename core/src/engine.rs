use crate::advice::{self, AuditInput, AuditResult};
use crate::entitlements::{self, Tier};
use crate::metrics::{self, Source, WindowStatus};
use crate::model::{EventKind, Tool, UsageEvent};
use crate::plans::PlanTable;
use crate::pricing::PriceTable;
use crate::runaway::{self, RunawayConfig};
use crate::scanner::{self, ScanReport};
use crate::settings::Settings;
use crate::snapshot::*;
use crate::store::Store;
use crate::sync::RemoteSummary;
use anyhow::Result;
use chrono::{DateTime, Duration, Utc};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};

const ALLOWANCE_TTL_MIN: i64 = 10;
const LIVE_FRESH_MIN: i64 = 15;

pub struct Engine {
    pub store: Store,
    pub prices: PriceTable,
    pub plans: PlanTable,
    pub settings: Settings,
    pub runaway_cfg: RunawayConfig,
    pub remote: Option<RemoteSummary>,
    pub live_errors: HashMap<Tool, String>,
    /// Probe the filesystem for installed tools (off for in-memory test engines).
    pub detect_fs: bool,
    allowances: HashMap<(Tool, &'static str), (f64, Source)>,
    allowances_at: Option<DateTime<Utc>>,
    audit_cache: Option<(DateTime<Utc>, Vec<AuditResult>, f64)>,
}

fn basename(p: &Option<String>) -> Option<String> {
    p.as_ref().map(|s| {
        Path::new(s).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| s.clone())
    })
}

impl Engine {
    pub fn with_store(store: Store) -> Engine {
        let settings = store
            .kv_get("settings")
            .ok()
            .flatten()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        let prices = store
            .kv_get("prices_json")
            .ok()
            .flatten()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        let plans = store
            .kv_get("plans_json")
            .ok()
            .flatten()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        Engine {
            store,
            prices,
            plans,
            settings,
            runaway_cfg: RunawayConfig::default(),
            remote: None,
            live_errors: HashMap::new(),
            detect_fs: true,
            allowances: HashMap::new(),
            allowances_at: None,
            audit_cache: None,
        }
    }

    pub fn open(path: &Path) -> Result<Engine> {
        Ok(Self::with_store(Store::open(path)?))
    }

    pub fn open_default() -> Result<Engine> {
        Self::open(&crate::paths::app_data_dir().join("usage.db"))
    }

    pub fn in_memory() -> Result<Engine> {
        let mut e = Self::with_store(Store::open_in_memory()?);
        e.detect_fs = false;
        Ok(e)
    }

    pub fn save_settings(&mut self) -> Result<()> {
        self.store.kv_set("settings", &serde_json::to_string(&self.settings)?)?;
        self.invalidate();
        Ok(())
    }

    pub fn invalidate(&mut self) {
        self.allowances_at = None;
        self.audit_cache = None;
    }

    /// Replace the price/plan tables with the backend's copy (no app release needed).
    pub fn update_tables(&mut self, prices_json: Option<&str>, plans_json: Option<&str>) -> Result<()> {
        if let Some(p) = prices_json {
            let t: PriceTable = serde_json::from_str(p)?;
            self.store.kv_set("prices_json", p)?;
            self.prices = t;
        }
        if let Some(p) = plans_json {
            let t: PlanTable = serde_json::from_str(p)?;
            self.store.kv_set("plans_json", p)?;
            self.plans = t;
        }
        self.invalidate();
        Ok(())
    }

    pub fn roots(&self, tool: Tool) -> Vec<PathBuf> {
        let mut r = crate::paths::default_roots(tool);
        r.extend(self.settings.tool(tool).extra_roots);
        r
    }

    pub fn detected(&self, tool: Tool) -> bool {
        (self.detect_fs && !crate::paths::detect(tool, &self.settings.tool(tool).extra_roots).is_empty())
            || self.store.has_events(tool).unwrap_or(false)
            || (tool == Tool::ClaudeCode && (self.settings.claude_web || self.store.has_gauges(tool).unwrap_or(false)))
    }

    /// Record the result of a live-gauge poll (Claude Code login or claude.ai).
    pub fn ingest_live(&mut self, tool: Tool, res: std::result::Result<Vec<crate::model::GaugeSample>, String>) {
        match res {
            Ok(g) => {
                self.live_errors.remove(&tool);
                let _ = self.store.insert_gauges(&g);
            }
            Err(e) => {
                self.live_errors.insert(tool, e);
            }
        }
    }

    /// Returns true once per expired web session, including across app restarts.
    pub fn ingest_web(&mut self, res: std::result::Result<Vec<crate::model::GaugeSample>, String>) -> Result<bool> {
        if !self.settings.claude_web { return Ok(false); }
        let expired = self.store.kv_get("claude_web_expired")?.as_deref() == Some("1");
        let signed_out = res.as_ref().err().is_some_and(|s| s == "signed_out");
        if signed_out && !expired {
            self.store.kv_set("claude_web_expired", "1")?;
        } else if expired && res.is_ok() {
            self.store.kv_set("claude_web_expired", "0")?;
        }
        if let Ok(samples) = &res {
            for sample in samples {
                if let Some(prev) = self.store.latest_gauge(Tool::ClaudeCode, &sample.window, sample.ts)? {
                    if sample.used_pct > prev.used_pct && prev.resets_at == sample.resets_at
                        && sample.ts - prev.ts <= Duration::minutes(30) {
                        self.store.kv_set("web_activity", &sample.ts.timestamp().to_string())?;
                        break;
                    }
                }
            }
        }
        self.ingest_live(Tool::ClaudeCode, res);
        Ok(signed_out && !expired && self.settings.alerts.notifications)
    }

/// The most recent Claude Code or Codex session from the last 24 hours, offered
    /// whatever the usage. Powers "Test handoff" in Check setup, so the flow can be
    /// tried without burning a plan limit to 90%.
    /// Project folders of Claude Code and Codex sessions in the last two weeks,
    /// most recent first. Used to suggest repositories for handoffs.
    pub fn recent_projects(&self, now: DateTime<Utc>) -> Result<Vec<PathBuf>> {
        let mut seen: Vec<(DateTime<Utc>, PathBuf)> = Vec::new();
        for tool in [Tool::ClaudeCode, Tool::Codex] {
            for e in self.store.events(Some(tool), now - Duration::days(14), now + Duration::seconds(1))? {
                let Some(p) = e.project.as_deref().map(PathBuf::from).filter(|p| p.is_absolute()) else { continue };
                match seen.iter_mut().find(|(_, q)| *q == p) {
                    Some(entry) => entry.0 = entry.0.max(e.ts),
                    None => seen.push((e.ts, p)),
                }
            }
        }
        seen.sort_by_key(|e| std::cmp::Reverse(e.0));
        Ok(seen.into_iter().map(|(_, p)| p).collect())
    }

    pub fn test_handoff_offers(&self, now: DateTime<Utc>) -> Result<Vec<crate::snapshot::HandoffOffer>> {
        let mut out = Vec::new();
        for tool in [Tool::ClaudeCode, Tool::Codex] {
            if !self.settings.tool(tool).enabled {
                continue;
            }
            let recent = self.store.events(Some(tool), now - Duration::hours(24), now + Duration::seconds(1))?;
            if let Some(e) = recent.iter().rev().find(|e| e.ts <= now) {
                let targets = crate::handoff::targets(tool);
                out.push(crate::snapshot::HandoffOffer { source: tool, session: e.session_id.clone(), targets });
            }
        }
        Ok(out)
    }

    pub fn diagnostics(&self, now: DateTime<Utc>) -> Vec<ToolDiagnostics> {
        let (day_start, day_end) = metrics::local_day_bounds(now);
        Tool::ALL
            .iter()
            .map(|&tool| {
                let checked = self.roots(tool);
                let found = crate::paths::detect(tool, &self.settings.tool(tool).extra_roots);
                let (total, last) = self.store.event_stats(tool).unwrap_or((0, None));
                let today = self
                    .store
                    .events(Some(tool), day_start, day_end)
                    .map(|v| v.iter().filter(|e| e.kind == EventKind::Assistant).count() as u64)
                    .unwrap_or(0);
                let window = if tool == Tool::Gemini { "daily" } else { "five_hour" };
                let g = self.store.latest_gauge(tool, window, now).ok().flatten();
                ToolDiagnostics {
                    tool,
                    label: tool.label().into(),
                    folders_found: found.iter().map(|p| p.display().to_string()).collect(),
                    folders_checked: checked.iter().map(|p| p.display().to_string()).collect(),
                    requests_total: total,
                    requests_today: today,
                    last_request_at: last,
                    gauge_source: g.as_ref().map(|g| g.source.clone()),
                    gauge_at: g.as_ref().map(|g| g.ts),
                    gauge_used_pct: g.as_ref().map(|g| g.used_pct),
                    live_error: self.live_errors.get(&tool).cloned(),
                }
            })
            .collect()
    }

    pub fn scan(&mut self) -> Result<ScanReport> {
        let mut total = ScanReport::default();
        for tool in Tool::ALL {
            if !self.settings.tool(tool).enabled {
                continue;
            }
            let roots = self.roots(tool);
            let r = scanner::scan_tool(&mut self.store, &self.prices, tool, &roots)?;
            total.files_seen += r.files_seen;
            total.files_read += r.files_read;
            total.events += r.events;
            total.gauges += r.gauges;
        }
        Ok(total)
    }

    /// Poll vendor gauges that aren't in the logs (Claude). Call about once a minute.
    pub fn refresh_live(&mut self, now: DateTime<Utc>) {
        if !self.settings.live_gauges || !self.settings.tool(Tool::ClaudeCode).enabled {
            return;
        }
        if self.settings.tool(Tool::ClaudeCode).plan_id.as_deref() == Some("api") {
            return;
        }
        let res = crate::live::fetch_claude_gauges(now).map_err(|e| e.to_string());
        self.ingest_live(Tool::ClaudeCode, res);
    }

    // ---------- allowances ----------

    fn calibrate(&self, tool: Tool, window: &'static str, now: DateTime<Utc>) -> Option<f64> {
        let g = self.store.latest_gauge(tool, window, now).ok()??;
        let key = format!("calib:{}:{}", tool.as_str(), window);
        let prev: Option<f64> = self.store.kv_get(&key).ok().flatten().and_then(|s| s.parse().ok());
        if now - g.ts > Duration::minutes(LIVE_FRESH_MIN) || g.used_pct < 10.0 {
            return prev;
        }
        let (Some(resets), Some(mins)) = (g.resets_at, g.window_minutes) else { return prev };
        let start = resets - Duration::minutes(mins as i64);
        let cost: f64 = self
            .store
            .events(Some(tool), start, g.ts + Duration::seconds(1))
            .ok()?
            .iter()
            .map(|e| e.cost_usd)
            .sum();
        if cost <= 0.0 {
            return prev;
        }
        let est = cost / (g.used_pct / 100.0);
        let v = match prev {
            Some(p) => p * 0.7 + est * 0.3,
            None => est,
        };
        let _ = self.store.kv_set(&key, &format!("{v:.4}"));
        Some(v)
    }

    fn refresh_allowances(&mut self, now: DateTime<Utc>) -> Result<()> {
        if let Some(at) = self.allowances_at {
            if now - at < Duration::minutes(ALLOWANCE_TTL_MIN) && now >= at {
                return Ok(());
            }
        }
        self.allowances.clear();
        for tool in [Tool::ClaudeCode, Tool::Codex] {
            let ts = self.settings.tool(tool);
            let hist = self.store.events(Some(tool), now - Duration::days(30), now)?;
            let five = ts
                .five_hour_allowance_usd
                .map(|a| (a, Source::Estimate))
                .or_else(|| self.calibrate(tool, "five_hour", now).map(|a| (a, Source::Estimate)))
                .or_else(|| metrics::historical_peak_block(&hist, now).map(|a| (a, Source::Estimate)));
            if let Some(a) = five {
                self.allowances.insert((tool, "five_hour"), a);
            }
            let week = ts
                .weekly_allowance_usd
                .map(|a| (a, Source::Estimate))
                .or_else(|| self.calibrate(tool, "weekly", now).map(|a| (a, Source::Estimate)))
                .or_else(|| metrics::historical_peak_week(&hist, now).map(|a| (a, Source::Estimate)));
            if let Some(a) = week {
                self.allowances.insert((tool, "weekly"), a);
            }
        }
        self.allowances_at = Some(now);
        Ok(())
    }

    // ---------- windows ----------

    fn fresh_gauge(&self, tool: Tool, window: &str, now: DateTime<Utc>, max_age: Duration) -> Option<crate::model::GaugeSample> {
        let g = self.store.latest_gauge(tool, window, now).ok()??;
        let still_meaningful = match g.resets_at {
            // a reading whose window has since reset still tells us the window is empty
            Some(_) => now - g.ts <= max_age.max(Duration::hours(24 * 7)),
            None => now - g.ts <= max_age,
        };
        still_meaningful.then_some(g)
    }

    fn windows_for(&self, tool: Tool, recent: &[UsageEvent], now: DateTime<Utc>) -> Vec<WindowStatus> {
        let mut out = Vec::new();
        match tool {
            Tool::ClaudeCode | Tool::Codex => {
                let max_age = if tool == Tool::Codex { Duration::hours(6) } else { Duration::minutes(LIVE_FRESH_MIN) };
                let live_ok = |g: &crate::model::GaugeSample| tool == Tool::Codex || now - g.ts <= Duration::minutes(LIVE_FRESH_MIN);
                // 5-hour
                match self.fresh_gauge(tool, "five_hour", now, max_age).filter(|g| live_ok(g)) {
                    Some(g) => out.push(metrics::gauge_status(&g, now)),
                    None => {
                        let block = metrics::current_block(recent, now);
                        let used = block.map(|b| b.2).unwrap_or(0.0);
                        let resets = block.map(|b| b.1);
                        out.push(metrics::usd_status("five_hour", used, self.allowances.get(&(tool, "five_hour")).cloned(), resets));
                    }
                }
                // weekly
                match self.fresh_gauge(tool, "weekly", now, max_age).filter(|g| live_ok(g)) {
                    Some(g) => out.push(metrics::gauge_status(&g, now)),
                    None => {
                        if let Some(a) = self.allowances.get(&(tool, "weekly")).cloned() {
                            let from = now - Duration::days(7);
                            let used: f64 = recent.iter().filter(|e| e.ts >= from).map(|e| e.cost_usd).sum();
                            out.push(metrics::usd_status("weekly", used, Some(a), None));
                        }
                    }
                }
                if tool == Tool::ClaudeCode {
                    if let Some(g) = self.fresh_gauge(tool, "weekly_opus", now, Duration::minutes(LIVE_FRESH_MIN)) {
                        if now - g.ts <= Duration::minutes(LIVE_FRESH_MIN) {
                            out.push(metrics::gauge_status(&g, now));
                        }
                    }
                }
            }
            Tool::Gemini => {
                let (start, end) = metrics::gemini_day_bounds(now);
                let used = recent.iter().filter(|e| e.kind == EventKind::Assistant && e.ts >= start && e.ts < end).count() as u64;
                let allowance = self.gemini_daily_allowance();
                let mut w = WindowStatus::from_pct("daily", used as f64 / allowance as f64 * 100.0, Some(end), Source::Estimate);
                w.used_requests = Some(used);
                w.allowance_requests = Some(allowance);
                out.push(w);
            }
        }
        out
    }

    fn gemini_daily_allowance(&self) -> u64 {
        let plan_id = self.settings.tool(Tool::Gemini).plan_id.unwrap_or_else(|| "free".into());
        self.plans.get(Tool::Gemini, &plan_id).and_then(|p| p.daily_requests).unwrap_or(1000)
    }

    fn pct_rate(&self, tool: Tool, w: &WindowStatus, recent: &[UsageEvent], now: DateTime<Utc>) -> Option<f64> {
        if w.source == Source::Learning {
            return None;
        }
        if tool == Tool::Gemini {
            let alloc = w.allowance_requests? as f64;
            return Some(metrics::requests_per_min(recent, now) / alloc * 100.0);
        }
        let burn = metrics::burn_usd_per_min(recent, now);
        if w.source == Source::Live {
            let samples = self.store.gauges(tool, &w.key, now - Duration::minutes(metrics::BURN_WINDOW_MIN), now + Duration::seconds(1)).ok()?;
            if let Some(r) = metrics::gauge_pct_per_min(&samples, now).filter(|r| *r > 0.0) {
                return Some(r);
            }
            let key: &'static str = if w.key == "weekly" { "weekly" } else { "five_hour" };
            let (alloc, _) = self.allowances.get(&(tool, key)).cloned()?;
            return Some(burn / alloc * 100.0);
        }
        let alloc = w.allowance_usd?;
        Some(burn / alloc * 100.0)
    }

    // ---------- snapshot ----------

    pub fn snapshot(&mut self, now: DateTime<Utc>) -> Result<Snapshot> {
        self.refresh_allowances(now)?;
        let ent = entitlements::resolve(&self.store, now);
        let (day_start, day_end) = metrics::local_day_bounds(now);
        let amber = if ent.paid() { self.settings.alerts.amber_pct } else { 20.0 };
        let red = if ent.paid() { self.settings.alerts.red_pct } else { 5.0 };

        let mut tools = Vec::new();
        let mut recent_by_tool: BTreeMap<Tool, Vec<UsageEvent>> = BTreeMap::new();
        for tool in Tool::ALL {
            let ts = self.settings.tool(tool);
            let recent = self.store.events(Some(tool), now - Duration::days(8), now + Duration::minutes(1))?;
            let detected = self.detected(tool);
            let windows = if ts.enabled && detected { self.windows_for(tool, &recent, now) } else { vec![] };
            let binding = windows
                .iter()
                .filter(|w| w.source != Source::Learning)
                .min_by(|a, b| a.headroom_pct.partial_cmp(&b.headroom_pct).unwrap());
            let headroom = binding.map(|w| w.headroom_pct);
            let source = binding.map(|w| w.source).unwrap_or(Source::Learning);
            let color = match headroom {
                None => "grey",
                Some(h) if h <= red => "red",
                Some(h) if h <= amber => "amber",
                Some(_) => "green",
            };
            let active = metrics::is_active(&recent, now);
            let minutes_left = if active {
                windows
                    .iter()
                    .filter_map(|w| self.pct_rate(tool, w, &recent, now).and_then(|r| metrics::minutes_to_cutoff(w.headroom_pct, r)))
                    .fold(None, |m: Option<f64>, x| Some(m.map_or(x, |m| m.min(x))))
                    .filter(|m| *m < 12.0 * 60.0)
                    .map(metrics::round5)
            } else {
                None
            };
            let today: Vec<&UsageEvent> = recent.iter().filter(|e| e.ts >= day_start && e.ts < day_end).collect();
            let plan = ts.plan_id.as_ref().and_then(|id| self.plans.get(tool, id));
            let reconnect = tool == Tool::ClaudeCode && self.settings.claude_web
                && self.store.kv_get("claude_web_expired")?.as_deref() == Some("1");
            let live_note = if tool != Tool::ClaudeCode || !ts.enabled || ts.plan_id.as_deref() == Some("api") {
                None
            } else if reconnect {
                Some("Reconnect".to_owned())
            } else if self.live_errors.contains_key(&tool) {
                Some("Offline".to_owned())
            } else if self.settings.claude_web || self.settings.live_gauges {
                let last = ["five_hour", "weekly", "weekly_opus"].iter()
                    .filter_map(|w| self.store.latest_gauge(tool, w, now).ok().flatten())
                    .map(|g| g.ts).max();
                last.filter(|ts| now - *ts > Duration::minutes(3))
                    .map(|ts| format!("Updated {} min ago", (now - ts).num_minutes()))
            } else { None };
            tools.push(ToolStatus {
                tool,
                label: tool.label().into(),
                detected,
                enabled: ts.enabled,
                plan_id: ts.plan_id.clone(),
                plan_name: plan.map(|p| p.name.clone()),
                headroom_pct: headroom,
                color: color.into(),
                source,
                windows,
                minutes_left,
                active,
                today_cost_usd: today.iter().map(|e| e.cost_usd).sum(),
                today_requests: today.iter().filter(|e| e.kind == EventKind::Assistant).count() as u64,
                burn_usd_per_min: metrics::burn_usd_per_min(&recent, now),
                locked: false,
                live_error: self.live_errors.get(&tool).cloned(),
                reconnect,
                live_note,
                needs_connect: tool == Tool::ClaudeCode
                    && ts.enabled
                    && ts.plan_id.as_deref() != Some("api")
                    && source != Source::Live,
            });
            recent_by_tool.insert(tool, recent);
        }

        // Active tool: most recent use in the last 30 minutes, else the primary tool,
        // else the most recently used tool, else the first detected one.
        let last_use = recent_by_tool
            .values()
            .flat_map(|v| v.iter().rev().find(|e| e.kind == EventKind::Assistant))
            .max_by_key(|e| e.ts)
            .cloned();
        let active_tool = last_use
            .as_ref()
            .filter(|e| now - e.ts <= Duration::minutes(30))
            .map(|e| e.tool)
            .or(self.settings.primary_tool)
            .or_else(|| last_use.as_ref().map(|e| e.tool))
            .or_else(|| tools.iter().find(|t| t.detected && t.enabled).map(|t| t.tool));

        // Today
        let all_today: Vec<&UsageEvent> = recent_by_tool
            .values()
            .flat_map(|v| v.iter())
            .filter(|e| e.ts >= day_start && e.ts < day_end && e.kind == EventKind::Assistant)
            .collect();
        let mut by_session: HashMap<(Tool, String), (f64, Option<String>)> = HashMap::new();
        for e in &all_today {
            let s = by_session.entry((e.tool, e.session_id.clone())).or_insert((0.0, None));
            s.0 += e.cost_usd;
            if e.project.is_some() {
                s.1 = e.project.clone();
            }
        }
        let top_session = by_session
            .iter()
            .max_by(|a, b| a.1 .0.partial_cmp(&b.1 .0).unwrap())
            .map(|((tool, sid), (c, p))| SessionCost { tool: *tool, session_id: sid.clone(), project_name: basename(p), cost_usd: *c });
        let today = TodayStats {
            cost_usd: all_today.iter().map(|e| e.cost_usd).sum(),
            sessions: by_session.len(),
            top_session,
        };

        // Session meter: the live session.
        let session_meter = last_use.as_ref().filter(|e| now - e.ts <= Duration::minutes(metrics::ACTIVE_GAP_MIN)).map(|last| {
            let evs: Vec<&UsageEvent> = recent_by_tool[&last.tool]
                .iter()
                .filter(|e| e.session_id == last.session_id && e.ts >= now - Duration::hours(12))
                .collect();
            let started = evs.first().map(|e| e.ts).unwrap_or(last.ts);
            let owned: Vec<UsageEvent> = evs.iter().map(|e| (*e).clone()).collect();
            SessionMeter {
                tool: last.tool,
                session_id: last.session_id.clone(),
                project_name: basename(&last.project),
                cost_usd: evs.iter().map(|e| e.cost_usd).sum(),
                started_at: started,
                minutes: (now - started).num_minutes(),
                burn_usd_per_min: metrics::burn_usd_per_min(&owned, now),
            }
        });

        // Alerts
        let mut alerts: Vec<Alert> = Vec::new();
        if self.settings.alerts.runaway_alerts {
            for (tool, recent) in &recent_by_tool {
                let baseline = runaway::baseline_5m(recent, now, self.runaway_cfg.baseline_floor_usd);
                let active_sessions: BTreeSet<&str> = recent
                    .iter()
                    .filter(|e| now - e.ts <= Duration::minutes(metrics::ACTIVE_GAP_MIN))
                    .map(|e| e.session_id.as_str())
                    .collect();
                for sid in active_sessions {
                    let sess: Vec<UsageEvent> = recent
                        .iter()
                        .filter(|e| e.session_id == sid && e.ts >= now - Duration::hours(2))
                        .cloned()
                        .collect();
                    if let Some(sig) = runaway::detect(&sess, baseline, now, &self.runaway_cfg) {
                        let id = format!("run:{}:{}:{}", tool.as_str(), sid, now.timestamp() / 1800);
                        if self.dismissed(&id) {
                            continue;
                        }
                        let action = if self.settings.alerts.one_click_stop {
                            AlertAction { kind: "stop".into(), label: "Stop".into(), target_tool: None }
                        } else {
                            AlertAction { kind: "dismiss".into(), label: "Dismiss".into(), target_tool: None }
                        };
                        alerts.push(Alert {
                            id,
                            level: "red".into(),
                            kind: "runaway".into(),
                            tool: *tool,
                            message: sig.message.clone(),
                            action,
                            session_id: Some(sig.session_id.clone()),
                            project: sig.project.clone(),
                            burn_usd_per_min: Some(sig.burn_usd_per_min),
                        });
                    }
                }
            }
        }
        for t in &tools {
            let Some(h) = t.headroom_pct else { continue };
            let level = if h <= red {
                "red"
            } else if h <= amber {
                "amber"
            } else {
                continue;
            };
            let w = t.windows.iter().filter(|w| w.source != Source::Learning).min_by(|a, b| a.headroom_pct.partial_cmp(&b.headroom_pct).unwrap());
            let resets = w.and_then(|w| w.resets_at).map(|r| r.timestamp()).unwrap_or(0);
            let id = format!("low:{}:{}:{}:{}", t.tool.as_str(), w.map(|w| w.key.as_str()).unwrap_or(""), level, resets);
            if self.dismissed(&id) {
                continue;
            }
            let alt = tools
                .iter()
                .filter(|o| o.tool != t.tool && o.detected && o.enabled && o.headroom_pct.unwrap_or(0.0) >= 30.0)
                .max_by(|a, b| a.headroom_pct.partial_cmp(&b.headroom_pct).unwrap());
            let action = match alt {
                Some(a) if ent.paid() => AlertAction { kind: "switch".into(), label: format!("Switch to {}", a.label), target_tool: Some(a.tool) },
                _ => AlertAction { kind: "dismiss".into(), label: "Dismiss".into(), target_tool: None },
            };
            let window_label = w.map(|w| w.label.to_lowercase()).unwrap_or_default();
            let msg = match t.minutes_left {
                Some(m) => format!("{} {} limit {:.0}% used, about {} min left at this pace", t.label, window_label, 100.0 - h, m),
                None => format!("{} {} limit {:.0}% used", t.label, window_label, 100.0 - h),
            };
            alerts.push(Alert {
                id,
                level: level.into(),
                kind: "low_headroom".into(),
                tool: t.tool,
                message: msg,
                action,
                session_id: None,
                project: None,
                burn_usd_per_min: None,
            });
        }
        alerts.sort_by_key(|a| (a.level != "red", a.kind != "runaway"));

        // Nudge
        let today_owned: Vec<UsageEvent> = all_today.iter().map(|e| (*e).clone()).collect();
        let day_key = now.with_timezone(&chrono::Local).format("%Y-%m-%d").to_string();
        let nudge = advice::model_nudge(&today_owned, &self.prices, &day_key).filter(|n| !self.dismissed(&format!("nudge:{}", n.id)));

        // Audit + projected saving (cached hourly)
        let (audit, projected) = self.audit_and_projection(now)?;

        let savings: f64 = self.store.savings_since(metrics::month_start_local(now))?.iter().map(|s| s.amount_usd).sum();

        let mut snap = Snapshot {
            handoffs: vec![],
            gauge_history: vec![],
            idle_hidden: self.settings.onboarded && self.settings.hide_when_idle
                && !recent_by_tool.values().flatten().any(|e| e.ts > now - Duration::minutes(30))
                && !self.store.kv_get("web_activity")?.and_then(|s| s.parse::<i64>().ok())
                    .is_some_and(|ts| ts > (now - Duration::minutes(30)).timestamp()),
            generated_at: now,
            onboarded: self.settings.onboarded,
            entitlements: ent.clone(),
            active_tool,
            tools,
            today,
            session_meter,
            alerts,
            nudge,
            savings_month_usd: Some(savings.max(0.0) + 0.0),
            audit,
            projected_saving_usd: projected,
            locked: vec![],
            remote: self.remote.clone(),
        };
        if !ent.paid() {
            self.apply_free_limits(&mut snap);
        }
        if let Some(tool) = snap.tools.iter().find(|t| Some(t.tool) == snap.active_tool && t.enabled && !t.locked) {
            let binding = tool.windows.iter().filter(|w| w.source != Source::Learning)
                .min_by(|a, b| a.headroom_pct.total_cmp(&b.headroom_pct));
            if let Some(w) = binding {
                let samples = self.store.gauges(tool.tool, &w.key, now - Duration::hours(24), now + Duration::milliseconds(1))?;
                for g in samples {
                    let point = crate::snapshot::GaugePoint {
                        at: g.ts.timestamp_millis(), used: g.used_pct,
                        reset_at: g.resets_at.map(|t| t.timestamp_millis()),
                    };
                    // One observation per five-minute bucket, preserving window boundaries.
                    if let Some(last) = snap.gauge_history.last_mut() {
                        if last.at / 300_000 == point.at / 300_000 && last.reset_at == point.reset_at {
                            *last = point;
                            continue;
                        }
                    }
                    snap.gauge_history.push(point);
                }
            }
        }
        for t in snap.tools.iter().filter(|t| t.enabled && !t.locked && t.tool != Tool::Gemini) {
            let near_limit = t.windows.iter().any(|w| matches!(w.key.as_str(), "five_hour" | "weekly")
                && w.source != Source::Learning && w.headroom_pct <= 10.0);
            let red = snap.alerts.iter().any(|a| a.tool == t.tool && a.kind == "low_headroom" && a.level == "red");
            if !near_limit && !red { continue; }
            if let Some(event) = recent_by_tool[&t.tool].iter().rev()
                .find(|e| e.ts <= now && e.ts >= now - Duration::hours(2)) {
                let targets = crate::handoff::targets(t.tool);
                snap.handoffs.push(crate::snapshot::HandoffOffer { source: t.tool, session: event.session_id.clone(), targets });
            }
        }
        Ok(snap)
    }

    fn apply_free_limits(&self, s: &mut Snapshot) {
        let primary = self
            .settings
            .primary_tool
            .or(s.active_tool)
            .or_else(|| s.tools.iter().find(|t| t.detected).map(|t| t.tool));
        for t in s.tools.iter_mut() {
            t.minutes_left = None;
            if Some(t.tool) != primary {
                t.locked = true;
                t.windows.clear();
                t.headroom_pct = None;
                t.color = "grey".into();
                t.today_cost_usd = 0.0;
                t.today_requests = 0;
            }
        }
        s.active_tool = primary;
        s.session_meter = None;
        s.alerts.retain(|a| a.kind == "low_headroom" && Some(a.tool) == primary);
        s.nudge = None;
        s.savings_month_usd = None;
        s.audit.clear();
        s.today.top_session = None;
        s.locked = ["time_to_cutoff", "switchboard", "runaway", "session_meter", "model_nudge", "plan_audit", "savings", "custom_alerts"]
            .iter()
            .map(|s| s.to_string())
            .collect();
    }

    fn audit_and_projection(&mut self, now: DateTime<Utc>) -> Result<(Vec<AuditResult>, f64)> {
        if let Some((at, a, p)) = &self.audit_cache {
            if now - *at < Duration::hours(1) && now >= *at {
                return Ok((a.clone(), *p));
            }
        }
        let mut results = Vec::new();
        for tool in Tool::ALL {
            if let Some(r) = self.audit_tool(tool, now)? {
                results.push(r);
            }
        }
        // Projected monthly saving = audit savings + small-task model waste extrapolated from the last 7 days.
        let week = self.store.events(None, now - Duration::days(7), now)?;
        let mut waste = 0.0;
        for e in week.iter().filter(|e| e.kind == EventKind::Assistant && e.output_tokens <= advice::SMALL_OUTPUT_TOKENS) {
            if let Some(rule) = e.model.as_deref().and_then(|m| self.prices.nudge_for(m)) {
                let alt = self.prices.cost_for(&rule.to, e.input_tokens, e.output_tokens, e.cache_write_tokens, e.cache_read_tokens);
                waste += (e.cost_usd - alt).max(0.0);
            }
        }
        // Model waste only turns into real dollars on API billing; on a subscription it is allowance.
        let api_billed = |t: Tool| self.settings.tool(t).plan_id.as_deref() == Some("api");
        let model_waste = if Tool::ALL.iter().any(|t| api_billed(*t)) { waste / 7.0 * 30.0 } else { 0.0 };
        let projected = results.iter().map(|r| r.monthly_saving_usd).sum::<f64>() + model_waste;
        self.audit_cache = Some((now, results.clone(), projected));
        Ok((results, projected))
    }

    pub fn audit_tool(&self, tool: Tool, now: DateTime<Utc>) -> Result<Option<AuditResult>> {
        let ts = self.settings.tool(tool);
        let Some(plan_id) = ts.plan_id.clone() else { return Ok(None) };
        let Some(plan) = self.plans.get(tool, &plan_id).cloned() else { return Ok(None) };
        if plan.metered || !ts.enabled {
            return Ok(None);
        }
        let tiers: Vec<_> = self.plans.tiers(tool).into_iter().cloned().collect();
        let this_week = metrics::week_start(now);
        let events = self.store.events(Some(tool), now - Duration::days(35), now)?;

        let mut weekly_peaks = Vec::new();
        let mut cap_hit_days = Vec::new();
        for k in (1..=4).rev() {
            let ws = this_week - Duration::days(7 * k);
            let we = ws + Duration::days(7);
            let peak = match tool {
                Tool::Gemini => {
                    let alloc = plan.daily_requests.unwrap_or(1000) as f64;
                    let mut per_day: BTreeMap<chrono::NaiveDate, u64> = BTreeMap::new();
                    for e in events.iter().filter(|e| e.kind == EventKind::Assistant && e.ts >= ws && e.ts < we) {
                        *per_day.entry(e.ts.with_timezone(&chrono_tz::America::Los_Angeles).date_naive()).or_default() += 1;
                    }
                    if k <= 2 {
                        cap_hit_days.push(per_day.values().filter(|n| **n as f64 >= alloc * 0.98).count() as u32);
                    }
                    let first = self.store.first_event_ts(Some(tool))?;
                    if first.map(|f| f <= ws).unwrap_or(false) {
                        Some(per_day.values().map(|n| *n as f64 / alloc * 100.0).fold(0.0, f64::max))
                    } else {
                        None
                    }
                }
                _ => {
                    let g = self.store.gauges(tool, "weekly", ws, we)?;
                    if k <= 2 {
                        let mut days = BTreeSet::new();
                        for w in ["five_hour", "weekly"] {
                            for s in self.store.gauges(tool, w, ws, we)? {
                                if s.used_pct >= 98.0 {
                                    days.insert(s.ts.date_naive());
                                }
                            }
                        }
                        cap_hit_days.push(days.len() as u32);
                    }
                    if !g.is_empty() {
                        Some(g.iter().map(|s| s.used_pct).fold(0.0, f64::max))
                    } else if let Some(a) = ts.weekly_allowance_usd {
                        let c: f64 = events.iter().filter(|e| e.ts >= ws && e.ts < we).map(|e| e.cost_usd).sum();
                        Some(c / a * 100.0)
                    } else {
                        None
                    }
                }
            };
            weekly_peaks.push(peak);
        }
        let active_days: BTreeSet<chrono::NaiveDate> = events
            .iter()
            .filter(|e| e.kind == EventKind::Assistant && e.ts >= now - Duration::days(30))
            .map(|e| e.ts.with_timezone(&chrono::Local).date_naive())
            .collect();
        let data_days = self.store.first_event_ts(Some(tool))?.map(|f| (now - f).num_days()).unwrap_or(0);
        let r = advice::audit(&AuditInput {
            tool,
            plan,
            tiers,
            weekly_peaks,
            cap_hit_days,
            active_days_30: active_days.len() as u32,
            data_days,
        });
        Ok(Some(r))
    }

    // ---------- user actions ----------

    fn dismissed(&self, id: &str) -> bool {
        self.store.kv_get(&format!("dismissed:{id}")).ok().flatten().is_some()
    }

    pub fn dismiss(&self, id: &str) -> Result<()> {
        self.store.kv_set(&format!("dismissed:{id}"), &Utc::now().to_rfc3339())
    }

    pub fn accept_nudge(&self, nudge: &advice::Nudge) -> Result<()> {
        self.store.add_saving(Utc::now(), "model_switch", nudge.saving_usd, &nudge.message)?;
        self.dismiss(&format!("nudge:{}", nudge.id))
    }

    /// User accepted a plan recommendation: record the monthly saving and switch the plan setting.
    pub fn accept_audit(&mut self, tool: Tool) -> Result<()> {
        let now = Utc::now();
        if let Some(r) = self.audit_tool(tool, now)? {
            if r.monthly_saving_usd > 0.0 {
                self.store.add_saving(now, "plan", r.monthly_saving_usd, &r.headline)?;
            }
            let mut ts = self.settings.tool(tool);
            match r.action {
                advice::AuditAction::Downgrade | advice::AuditAction::Upgrade => ts.plan_id = r.target_plan_id.clone(),
                advice::AuditAction::Cancel => ts.enabled = false,
                _ => {}
            }
            self.settings.tools.insert(tool, ts);
            self.save_settings()?;
        }
        Ok(())
    }

    /// Stop a runaway session. Records the spend avoided (15 minutes at the current burn rate, estimated).
    pub fn stop_runaway(&self, alert: &Alert) -> Result<usize> {
        let n = crate::stop::interrupt_session(alert.tool, alert.project.as_deref())?;
        if n > 0 {
            let avoided = alert.burn_usd_per_min.unwrap_or(0.0) * 15.0;
            self.store.add_saving(Utc::now(), "runaway", avoided, &alert.message)?;
            self.dismiss(&alert.id)?;
        }
        Ok(n)
    }

    pub fn set_licence_tier(&self, tier: Tier) -> Result<()> {
        let rec = entitlements::LicenceRecord {
            key: "test".into(),
            instance_id: None,
            tier,
            last_validated: Utc::now(),
            status: "active".into(),
        };
        self.store.kv_set("licence", &serde_json::to_string(&rec)?)
    }
}
