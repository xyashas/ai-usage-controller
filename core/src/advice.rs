//! Money advice: the daily model-cost nudge and the monthly plan audit.

use crate::model::{EventKind, Tool, UsageEvent};
use crate::plans::Plan;
use crate::pricing::PriceTable;
use serde::Serialize;
use std::collections::BTreeMap;

/// A response counts as a "small task" when it wrote at most this many output tokens.
pub const SMALL_OUTPUT_TOKENS: u64 = 1_500;
pub const NUDGE_MIN_COUNT: usize = 30;
pub const NUDGE_MIN_SAVING_USD: f64 = 2.0;

#[derive(Clone, Debug, Serialize)]
pub struct Nudge {
    pub id: String,
    pub tool: Tool,
    pub count: usize,
    pub from_label: String,
    pub to_label: String,
    pub to_model: String,
    pub actual_usd: f64,
    pub alternative_usd: f64,
    pub saving_usd: f64,
    pub message: String,
}

/// Group the day's small responses by expensive model and price them at the cheaper model.
pub fn model_nudge(day_events: &[UsageEvent], prices: &PriceTable, day_key: &str) -> Option<Nudge> {
    struct Acc {
        tool: Tool,
        count: usize,
        actual: f64,
        alt: f64,
    }
    let mut groups: BTreeMap<String, Acc> = BTreeMap::new();
    for e in day_events.iter().filter(|e| e.kind == EventKind::Assistant && e.output_tokens <= SMALL_OUTPUT_TOKENS) {
        let Some(model) = &e.model else { continue };
        let Some(rule) = prices.nudge_for(model) else { continue };
        let alt = prices.cost_for(&rule.to, e.input_tokens, e.output_tokens, e.cache_write_tokens, e.cache_read_tokens);
        let a = groups.entry(rule.from.clone()).or_insert(Acc { tool: e.tool, count: 0, actual: 0.0, alt: 0.0 });
        a.count += 1;
        a.actual += e.cost_usd;
        a.alt += alt;
    }
    let (from, acc) = groups
        .into_iter()
        .filter(|(_, a)| a.count >= NUDGE_MIN_COUNT && a.actual - a.alt >= NUDGE_MIN_SAVING_USD)
        .max_by(|a, b| (a.1.actual - a.1.alt).partial_cmp(&(b.1.actual - b.1.alt)).unwrap())?;
    let rule = prices.nudges.iter().find(|r| r.from == from)?;
    let saving = acc.actual - acc.alt;
    Some(Nudge {
        id: format!("{day_key}:{from}"),
        tool: acc.tool,
        count: acc.count,
        from_label: rule.label_from.clone(),
        to_label: rule.label_to.clone(),
        to_model: rule.to.clone(),
        actual_usd: acc.actual,
        alternative_usd: acc.alt,
        saving_usd: saving,
        message: format!(
            "You ran {} small edits on {} today. {} would cost about ${:.0} less.",
            acc.count,
            rule.label_from,
            rule.label_to,
            saving.round().max(1.0)
        ),
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AuditAction {
    Keep,
    Downgrade,
    Upgrade,
    Cancel,
    NotEnoughData,
}

#[derive(Clone, Debug)]
pub struct AuditInput {
    pub tool: Tool,
    pub plan: Plan,
    /// Subscription tiers sorted by capacity ascending (includes the current plan).
    pub tiers: Vec<Plan>,
    /// Peak use of the current plan's weekly allowance, in %, for each of the last 4 complete weeks.
    /// None = no reading for that week.
    pub weekly_peaks: Vec<Option<f64>>,
    /// Days per week the user hit a cap, last 2 complete weeks (most recent last).
    pub cap_hit_days: Vec<u32>,
    pub active_days_30: u32,
    pub data_days: i64,
}

#[derive(Clone, Debug, Serialize)]
pub struct AuditResult {
    pub tool: Tool,
    pub current_plan: String,
    pub current_price: f64,
    pub action: AuditAction,
    pub target_plan_id: Option<String>,
    pub target_plan: Option<String>,
    pub monthly_saving_usd: f64,
    pub peak_used_pct: Option<f64>,
    pub headline: String,
    pub reasoning: Vec<String>,
}

pub const MIN_DATA_DAYS: i64 = 28;

pub fn audit(inp: &AuditInput) -> AuditResult {
    let peaks: Vec<f64> = inp.weekly_peaks.iter().flatten().cloned().collect();
    let peak = peaks.iter().cloned().fold(None, |m: Option<f64>, p| Some(m.map_or(p, |m| m.max(p))));
    let mut res = AuditResult {
        tool: inp.tool,
        current_plan: inp.plan.name.clone(),
        current_price: inp.plan.price,
        action: AuditAction::Keep,
        target_plan_id: None,
        target_plan: None,
        monthly_saving_usd: 0.0,
        peak_used_pct: peak,
        headline: String::new(),
        reasoning: vec![],
    };

    if inp.data_days < MIN_DATA_DAYS {
        res.action = AuditAction::NotEnoughData;
        res.headline = format!(
            "Collecting data: plan advice in {} days.",
            MIN_DATA_DAYS - inp.data_days.max(0)
        );
        res.reasoning.push("We only recommend a plan change after 4 weeks of usage.".into());
        return res;
    }

    if inp.active_days_30 < 4 && inp.plan.price > 0.0 {
        res.action = AuditAction::Cancel;
        res.monthly_saving_usd = inp.plan.price;
        res.headline = format!(
            "You opened {} on {} days this month. Cancelling saves you ${:.0}/month.",
            inp.tool.label(),
            inp.active_days_30,
            inp.plan.price
        );
        res.reasoning.push(format!("Active on {} of the last 30 days (threshold: fewer than 4).", inp.active_days_30));
        return res;
    }

    if inp.cap_hit_days.iter().any(|d| *d > 2) {
        if let Some(up) = inp.tiers.iter().find(|t| t.capacity > inp.plan.capacity) {
            res.action = AuditAction::Upgrade;
            res.target_plan_id = Some(up.id.clone());
            res.target_plan = Some(up.name.clone());
            res.monthly_saving_usd = 0.0;
            res.headline = format!(
                "You hit your {} cap on {} days in one week. {} costs ${:.0} more and stops the cut-offs.",
                inp.plan.name,
                inp.cap_hit_days.iter().max().unwrap(),
                up.name,
                up.price - inp.plan.price
            );
            res.reasoning.push("Hit a usage cap more than twice in a week.".into());
            return res;
        }
    }

    let lower = inp
        .tiers
        .iter()
        .filter(|t| t.capacity < inp.plan.capacity && t.price < inp.plan.price)
        .max_by(|a, b| a.capacity.partial_cmp(&b.capacity).unwrap());
    if let Some(lower) = lower {
        let full = inp.weekly_peaks.len() >= 4 && inp.weekly_peaks.iter().rev().take(4).all(|p| p.is_some());
        if full {
            let ratio = inp.plan.capacity / lower.capacity;
            let on_lower: Vec<f64> = inp.weekly_peaks.iter().rev().take(4).flatten().map(|p| p * ratio).collect();
            let max_on_lower = on_lower.iter().cloned().fold(0.0, f64::max);
            if max_on_lower < 60.0 {
                let saving = inp.plan.price - lower.price;
                res.action = AuditAction::Downgrade;
                res.target_plan_id = Some(lower.id.clone());
                res.target_plan = Some(lower.name.clone());
                res.monthly_saving_usd = saving;
                res.headline = format!(
                    "You used {:.0}% of {}. {} saves you ${:.0}/month.",
                    peak.unwrap_or(0.0),
                    inp.plan.name,
                    lower.name,
                    saving
                );
                res.reasoning.push(format!(
                    "Peak weekly use would have been {:.0}% of {} (threshold: under 60% for 4 weeks).",
                    max_on_lower, lower.name
                ));
                return res;
            }
            res.reasoning.push(format!(
                "On {} your peak week would have been {:.0}%, too close to the cap.",
                lower.name, max_on_lower
            ));
        } else {
            res.reasoning.push("Need 4 full weeks of live gauge readings to compare against a lower tier.".into());
        }
    }

    res.headline = match peak {
        Some(p) => format!("You used {:.0}% of {} at peak. Your plan fits.", p, inp.plan.name),
        None => format!("{} looks right for how you work.", inp.plan.name),
    };
    res
}
