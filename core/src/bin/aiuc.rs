//! `aiuc` — the engine in a terminal. Week-1 gate: compare these numbers with /usage.
//!
//!   aiuc paths                 where logs are read from on this machine
//!   aiuc status [--json]       headroom, burn rate, reset timers, today's cost
//!   aiuc watch                 refresh status every 5 seconds
//!   aiuc plan <tool> <plan>    set your plan (claude_code: pro|max5|max20|api ...)
//!   aiuc audit                 monthly plan audit
//!   aiuc trial                 start the 7-day Pro trial
//!   aiuc activate <key>        activate a Pro/Team licence key

use aiuc_core::metrics::Source;
use aiuc_core::{Engine, Snapshot, Tool};
use chrono::{Local, Utc};

fn bar(pct: f64, width: usize) -> String {
    let filled = ((pct / 100.0) * width as f64).round().clamp(0.0, width as f64) as usize;
    format!("{}{}", "█".repeat(filled), "░".repeat(width - filled))
}

fn color(c: &str, s: &str) -> String {
    let code = match c {
        "green" => "32",
        "amber" => "33",
        "red" => "31",
        _ => "90",
    };
    format!("\x1b[{code}m{s}\x1b[0m")
}

fn print_status(s: &Snapshot) {
    println!(
        "AI Usage Controller · {} · {:?}",
        s.generated_at.with_timezone(&Local).format("%H:%M:%S"),
        s.entitlements.tier
    );
    println!();
    for t in &s.tools {
        if !t.detected {
            println!("  {:<12} not found on this machine", t.label);
            continue;
        }
        if t.locked {
            println!("  {:<12} 🔒 Pro", t.label);
            continue;
        }
        let head = t.headroom_pct.map(|h| format!("{h:>5.1}% left")).unwrap_or_else(|| " learning".into());
        let src = match t.source {
            Source::Live => "live",
            Source::Estimate => "estimate",
            Source::Learning => "learning",
        };
        let mins = t.minutes_left.map(|m| format!("  about {m} min left at this pace")).unwrap_or_default();
        println!(
            "  {:<12} {} {}  ({}){}",
            t.label,
            color(&t.color, &bar(t.headroom_pct.unwrap_or(0.0), 20)),
            head,
            src,
            mins
        );
        for w in &t.windows {
            let reset = w
                .resets_at
                .map(|r| {
                    let d = r - Utc::now();
                    let h = d.num_minutes() / 60;
                    let m = d.num_minutes() % 60;
                    format!("resets in {h}h {m:02}m")
                })
                .unwrap_or_else(|| "resets on next use".into());
            let detail = match (w.used_usd, w.allowance_usd, w.used_requests, w.allowance_requests) {
                (_, _, Some(u), Some(a)) => format!("{u}/{a} requests"),
                (Some(u), Some(a), _, _) => format!("${u:.2} of ~${a:.2}"),
                (Some(u), None, _, _) => format!("${u:.2} (allowance unknown yet)"),
                _ => String::new(),
            };
            println!("      {:<14} {:>5.1}% used  {}  {}", w.label, w.used_pct, reset, detail);
        }
        println!(
            "      today ${:.2} · {} requests · burn ${:.3}/min{}",
            t.today_cost_usd,
            t.today_requests,
            t.burn_usd_per_min,
            t.live_error.as_ref().map(|e| format!("\n      live gauge: {e}")).unwrap_or_default()
        );
    }
    println!();
    println!("  Today: ${:.2} across {} sessions", s.today.cost_usd, s.today.sessions);
    if let Some(top) = &s.today.top_session {
        println!("  Most expensive: {} ${:.2}", top.project_name.clone().unwrap_or_else(|| top.session_id.clone()), top.cost_usd);
    }
    if let Some(m) = &s.session_meter {
        println!("  Live session: ${:.2} over {} min ({})", m.cost_usd, m.minutes, m.project_name.clone().unwrap_or_default());
    }
    for a in &s.alerts {
        println!("  {} {}  [{}]", color(&a.level, "●"), a.message, a.action.label);
    }
    if let Some(n) = &s.nudge {
        println!("  💡 {}", n.message);
    }
    if let Some(sv) = s.savings_month_usd {
        println!("  Saved this month (estimated): ${sv:.2}");
    }
    if !s.locked.is_empty() {
        println!("  Pro unlocks: {}  (aiuc trial)", s.locked.join(", "));
    }
    println!("\n  Costs are API-list-price estimates; on a subscription they measure allowance, not money.");
}

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let cmd = args.first().map(String::as_str).unwrap_or("status");
    let mut engine = Engine::open_default()?;

    match cmd {
        "paths" => {
            for t in Tool::ALL {
                println!("{}:", t.label());
                for r in engine.roots(t) {
                    println!("  {} {}", if r.is_dir() { "✓" } else { "·" }, r.display());
                }
            }
            println!("database: {}", aiuc_core::paths::app_data_dir().join("usage.db").display());
        }
        "status" | "watch" => {
            let json = args.iter().any(|a| a == "--json");
            loop {
                let rep = engine.scan()?;
                engine.refresh_live(Utc::now());
                let s = engine.snapshot(Utc::now())?;
                if json {
                    println!("{}", serde_json::to_string_pretty(&s)?);
                } else {
                    if cmd == "watch" {
                        print!("\x1b[2J\x1b[H");
                    }
                    print_status(&s);
                    if cmd == "status" {
                        println!("  (scanned {} files, {} new records)", rep.files_seen, rep.events);
                    }
                }
                if cmd != "watch" {
                    break;
                }
                std::thread::sleep(std::time::Duration::from_secs(5));
            }
        }
        "plan" => {
            let (Some(tool), Some(plan)) = (args.get(1).and_then(|t| Tool::parse(t)), args.get(2)) else {
                eprintln!("usage: aiuc plan <claude_code|codex|gemini> <plan-id>");
                for t in Tool::ALL {
                    let ids: Vec<_> = engine.plans.plans(t).iter().map(|p| p.id.clone()).collect();
                    eprintln!("  {}: {}", t.as_str(), ids.join(" | "));
                }
                std::process::exit(2);
            };
            if engine.plans.get(tool, plan).is_none() {
                anyhow::bail!("unknown plan {plan} for {}", tool.as_str());
            }
            let mut ts = engine.settings.tool(tool);
            ts.plan_id = Some(plan.clone());
            engine.settings.tools.insert(tool, ts);
            if engine.settings.primary_tool.is_none() {
                engine.settings.primary_tool = Some(tool);
            }
            engine.save_settings()?;
            println!("{} plan set to {}", tool.label(), plan);
        }
        "audit" => {
            engine.scan()?;
            for t in Tool::ALL {
                if let Some(r) = engine.audit_tool(t, Utc::now())? {
                    println!("{}: {:?} — {}", t.label(), r.action, r.headline);
                    for why in r.reasoning {
                        println!("    · {why}");
                    }
                }
            }
        }
        "trial" => {
            aiuc_core::entitlements::start_trial(&engine.store, Utc::now())?;
            println!("Pro trial started: 7 days, no card.");
        }
        "activate" => {
            let key = args.get(1).ok_or_else(|| anyhow::anyhow!("usage: aiuc activate <licence-key>"))?;
            let host = sysinfo::System::host_name().unwrap_or_else(|| "device".into());
            let rec = aiuc_core::licence::activate(&engine.store, key, &host)?;
            println!("Activated: {:?}", rec.tier);
        }
        _ => {
            eprintln!("commands: paths | status [--json] | watch | plan <tool> <plan> | audit | trial | activate <key>");
            std::process::exit(2);
        }
    }
    Ok(())
}
