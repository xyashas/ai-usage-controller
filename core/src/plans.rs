use crate::model::Tool;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const DEFAULT_PLANS: &str = include_str!("../data/plans.json");

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Plan {
    pub id: String,
    pub name: String,
    pub price: f64,
    pub capacity: f64,
    #[serde(default)]
    pub metered: bool,
    #[serde(default)]
    pub daily_requests: Option<u64>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PlanTable {
    pub version: String,
    pub tools: BTreeMap<String, Vec<Plan>>,
}

impl Default for PlanTable {
    fn default() -> Self {
        serde_json::from_str(DEFAULT_PLANS).expect("embedded plans.json is valid")
    }
}

impl PlanTable {
    pub fn plans(&self, tool: Tool) -> &[Plan] {
        self.tools.get(tool.as_str()).map(|v| v.as_slice()).unwrap_or(&[])
    }

    pub fn get(&self, tool: Tool, id: &str) -> Option<&Plan> {
        self.plans(tool).iter().find(|p| p.id == id)
    }

    /// Subscription tiers (non-metered) sorted by capacity ascending.
    pub fn tiers(&self, tool: Tool) -> Vec<&Plan> {
        let mut v: Vec<&Plan> = self.plans(tool).iter().filter(|p| !p.metered && p.price > 0.0).collect();
        v.sort_by(|a, b| a.capacity.partial_cmp(&b.capacity).unwrap().then(a.price.partial_cmp(&b.price).unwrap()));
        v
    }
}
