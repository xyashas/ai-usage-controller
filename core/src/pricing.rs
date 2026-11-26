use crate::model::UsageEvent;
use serde::{Deserialize, Serialize};

pub const DEFAULT_PRICES: &str = include_str!("../data/prices.json");

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ModelPrice {
    pub pattern: String,
    pub input: f64,
    pub output: f64,
    #[serde(default)]
    pub cache_write: Option<f64>,
    #[serde(default)]
    pub cache_read: Option<f64>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct NudgeRule {
    pub from: String,
    pub to: String,
    pub label_from: String,
    pub label_to: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PriceTable {
    pub version: String,
    pub models: Vec<ModelPrice>,
    #[serde(default)]
    pub nudges: Vec<NudgeRule>,
}

impl Default for PriceTable {
    fn default() -> Self {
        serde_json::from_str(DEFAULT_PRICES).expect("embedded prices.json is valid")
    }
}

impl PriceTable {
    /// Longest pattern contained in the (lowercased) model name wins, so
    /// "claude-opus-4-5-20251101" matches "claude-opus-4-5", not "claude-opus-4".
    pub fn lookup(&self, model: &str) -> Option<&ModelPrice> {
        let m = model.to_ascii_lowercase();
        self.models
            .iter()
            .filter(|p| m.contains(&p.pattern))
            .max_by_key(|p| p.pattern.len())
    }

    pub fn cost_for(&self, model: &str, input: u64, output: u64, cache_write: u64, cache_read: u64) -> f64 {
        let Some(p) = self.lookup(model) else { return 0.0 };
        let per = |tokens: u64, price: f64| tokens as f64 * price / 1_000_000.0;
        per(input, p.input)
            + per(output, p.output)
            + per(cache_write, p.cache_write.unwrap_or(p.input * 1.25))
            + per(cache_read, p.cache_read.unwrap_or(p.input * 0.1))
    }

    pub fn cost(&self, ev: &UsageEvent) -> f64 {
        match &ev.model {
            Some(m) => self.cost_for(m, ev.input_tokens, ev.output_tokens, ev.cache_write_tokens, ev.cache_read_tokens),
            None => 0.0,
        }
    }

    pub fn nudge_for(&self, model: &str) -> Option<&NudgeRule> {
        let m = model.to_ascii_lowercase();
        self.nudges
            .iter()
            .filter(|r| m.contains(&r.from))
            .max_by_key(|r| r.from.len())
    }
}
