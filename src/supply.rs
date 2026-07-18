//! Hard supply cap for GRID credits.
//!
//! - **Max supply:** 10,000,000,000 GRID (10B)
//! - **Circulating** = total_minted − total_burned
//! - Burns (node unclaimed + idle wallet + voluntary) free room under the cap
//!   so emission never “fills up” permanently with dead coins.
//! - Prefer **exit to BTC / other crypto / fiat** over long-term GRID hoarding.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Hard ceiling on circulating GRID (10 billion).
pub const MAX_SUPPLY: f64 = 10_000_000_000.0;

/// Inactivity / unclaimed window before burn (days).
pub const BURN_DEADLINE_DAYS: i64 = 365;

const SUPPLY_FILE: &str = "supply.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SupplyState {
    pub max_supply: f64,
    /// Lifetime gross mint (never decreases).
    pub total_minted: f64,
    /// Lifetime burns (node + wallet + voluntary). Frees mint headroom.
    pub total_burned: f64,
    pub updated_at: String,
}

impl Default for SupplyState {
    fn default() -> Self {
        Self {
            max_supply: MAX_SUPPLY,
            total_minted: 0.0,
            total_burned: 0.0,
            updated_at: chrono::Utc::now().to_rfc3339(),
        }
    }
}

impl SupplyState {
    pub fn path_in(config_dir: &Path) -> PathBuf {
        config_dir.join(SUPPLY_FILE)
    }

    pub fn load(config_dir: &Path) -> Result<Self> {
        let p = Self::path_in(config_dir);
        if !p.exists() {
            return Ok(Self::default());
        }
        let raw = std::fs::read_to_string(&p)?;
        let mut s: Self = serde_json::from_str(&raw)?;
        if s.max_supply <= 0.0 {
            s.max_supply = MAX_SUPPLY;
        }
        Ok(s)
    }

    pub fn save(&self, config_dir: &Path) -> Result<()> {
        let p = Self::path_in(config_dir);
        if let Some(parent) = p.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let tmp = p.with_extension("json.tmp");
        let mut s = self.clone();
        s.updated_at = chrono::Utc::now().to_rfc3339();
        std::fs::write(&tmp, serde_json::to_string_pretty(&s)?)
            .with_context(|| format!("write {}", tmp.display()))?;
        std::fs::rename(&tmp, &p)?;
        Ok(())
    }

    pub fn circulating(&self) -> f64 {
        (self.total_minted - self.total_burned).max(0.0)
    }

    /// How much can still be minted without exceeding max supply.
    pub fn mint_headroom(&self) -> f64 {
        (self.max_supply - self.circulating()).max(0.0)
    }

    /// Cap a requested mint to remaining headroom. Returns actual mint amount.
    pub fn apply_mint(&mut self, requested: f64) -> f64 {
        if requested <= 0.0 || !requested.is_finite() {
            return 0.0;
        }
        let amt = requested.min(self.mint_headroom());
        if amt <= 0.0 {
            return 0.0;
        }
        self.total_minted += amt;
        amt
    }

    pub fn apply_burn(&mut self, amount: f64) {
        if amount <= 0.0 || !amount.is_finite() {
            return;
        }
        self.total_burned += amount;
    }
}

pub fn print_supply_banner(s: &SupplyState) {
    println!("  supply cap:    {:.0} GRID (hard)", s.max_supply);
    println!("  circulating:   {:.6} GRID", s.circulating());
    println!("  minted life:   {:.6}", s.total_minted);
    println!("  burned life:   {:.6}  (frees mint room)", s.total_burned);
    println!("  mint headroom: {:.6}", s.mint_headroom());
}

pub fn print_exit_advice() {
    println!();
    println!("  ★ BEST PRACTICE: exit value off GRID.");
    println!("    Prefer BTC (TSL), other crypto, or fiat — do not hoard forever.");
    println!("    Idle node mint AND idle wallet balances burn after {BURN_DEADLINE_DAYS} days.");
    println!("    Burns free headroom under the 10B cap so useful work can mint again.");
    println!();
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn mint_capped_and_burn_frees() {
        let dir = tempdir().unwrap();
        let mut s = SupplyState::default();
        // simulate near-full: mint almost all
        s.total_minted = MAX_SUPPLY - 100.0;
        s.total_burned = 0.0;
        assert!((s.mint_headroom() - 100.0).abs() < 1e-6);
        let got = s.apply_mint(500.0);
        assert!((got - 100.0).abs() < 1e-6);
        assert!((s.mint_headroom()).abs() < 1e-6);
        s.apply_burn(50.0);
        assert!((s.mint_headroom() - 50.0).abs() < 1e-6);
        s.save(dir.path()).unwrap();
        let s2 = SupplyState::load(dir.path()).unwrap();
        assert!((s2.circulating() - (MAX_SUPPLY - 50.0)).abs() < 1e-3);
    }
}
