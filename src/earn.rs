//! Local Phase 1 earn ledger (JSON). Genesis Earn / on-rail comes later.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct EarnLedger {
    pub balances: HashMap<String, f64>,
    pub total_minted: f64,
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
        std::fs::write(path, serde_json::to_string_pretty(self)?)
            .with_context(|| format!("write {}", path.display()))?;
        Ok(())
    }

    pub fn credit(&mut self, node_id: &str, amount: f64) {
        if amount <= 0.0 {
            return;
        }
        *self.balances.entry(node_id.to_string()).or_insert(0.0) += amount;
        self.total_minted += amount;
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
        l.credit("n1", 10.5);
        l.save(&path).unwrap();
        let l2 = EarnLedger::load(&path).unwrap();
        assert!((l2.balance("n1") - 10.5).abs() < 1e-9);
    }
}
