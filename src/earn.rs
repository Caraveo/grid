//! Phase 1 earn ledger — persistent off-chain credits (Genesis Earn on-rail later).
//!
//! Real local accounting for verified PoR work. Hard **10B supply cap** is enforced
//! via [`crate::supply`]. Burns free mint headroom. Prefer exit to BTC/fiat.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::supply::SupplyState;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EarnLedger {
    pub balances: HashMap<String, f64>,
    pub total_minted: f64,
    #[serde(default)]
    pub events: Vec<EarnEvent>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EarnEvent {
    pub at: String,
    pub node_id: String,
    pub job_id: String,
    pub amount: f64,
    pub commitment: String,
}

impl EarnLedger {
    pub fn path_in(dir: &Path) -> PathBuf {
        dir.join("earn.json")
    }

    pub fn load(path: &Path) -> Result<Self> {
        if !path.exists() {
            return Ok(Self::default());
        }
        let raw = std::fs::read_to_string(path)?;
        Ok(serde_json::from_str(&raw)?)
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(p) = path.parent() {
            std::fs::create_dir_all(p)?;
        }
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, serde_json::to_string_pretty(self)?)
            .with_context(|| format!("write {}", tmp.display()))?;
        std::fs::rename(&tmp, path)?;
        Ok(())
    }

    pub fn credit(&mut self, node_id: &str, amount: f64) {
        if amount <= 0.0 {
            return;
        }
        *self.balances.entry(node_id.to_string()).or_insert(0.0) += amount;
        self.total_minted += amount;
    }

    /// Credit with hard 10B supply enforcement. Returns actual amount minted (may be 0).
    pub fn credit_job_capped(
        &mut self,
        config_dir: &Path,
        node_id: &str,
        job_id: &str,
        amount: f64,
        commitment: &str,
        at: impl Into<String>,
    ) -> Result<f64> {
        if amount <= 0.0 || !amount.is_finite() {
            return Ok(0.0);
        }
        let mut supply = SupplyState::load(config_dir).unwrap_or_default();
        let actual = supply.apply_mint(amount);
        if actual <= 0.0 {
            supply.save(config_dir)?;
            return Ok(0.0);
        }
        supply.save(config_dir)?;
        self.credit(node_id, actual);
        self.events.push(EarnEvent {
            at: at.into(),
            node_id: node_id.to_string(),
            job_id: job_id.to_string(),
            amount: actual,
            commitment: commitment.to_string(),
        });
        if self.events.len() > 10_000 {
            let drop_n = self.events.len() - 10_000;
            self.events.drain(0..drop_n);
        }
        Ok(actual)
    }

    pub fn credit_job(
        &mut self,
        node_id: &str,
        job_id: &str,
        amount: f64,
        commitment: &str,
        at: impl Into<String>,
    ) {
        if amount <= 0.0 {
            return;
        }
        self.credit(node_id, amount);
        self.events.push(EarnEvent {
            at: at.into(),
            node_id: node_id.to_string(),
            job_id: job_id.to_string(),
            amount,
            commitment: commitment.to_string(),
        });
        if self.events.len() > 10_000 {
            let drop_n = self.events.len() - 10_000;
            self.events.drain(0..drop_n);
        }
    }

    pub fn balance(&self, node_id: &str) -> f64 {
        self.balances.get(node_id).copied().unwrap_or(0.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn credit_persist() {
        let dir = tempdir().unwrap();
        let path = EarnLedger::path_in(dir.path());
        let mut l = EarnLedger::default();
        l.credit_job("n1", "job_1", 10.5, "abc", "t");
        l.save(&path).unwrap();
        let l2 = EarnLedger::load(&path).unwrap();
        assert!((l2.balance("n1") - 10.5).abs() < 1e-9);
        assert_eq!(l2.events.len(), 1);
    }
}
