//! Bitcoin as the Transact Security Layer.

use serde::{Deserialize, Serialize};

pub const BITCOIN_TSL: &str = "Bitcoin is the Transact Security Layer";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransactSecurityLayer {
    pub network: String,
    pub primary_exit: String,
    pub commitment_anchoring: bool,
}

impl Default for TransactSecurityLayer {
    fn default() -> Self {
        Self {
            network: "bitcoin".into(),
            primary_exit: "GRID→BTC".into(),
            commitment_anchoring: true,
        }
    }
}

impl TransactSecurityLayer {
    pub fn describe(&self) -> String {
        format!(
            "{BITCOIN_TSL} (network={}, exit={}, anchor={})",
            self.network, self.primary_exit, self.commitment_anchoring
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_bitcoin() {
        let t = TransactSecurityLayer::default();
        assert_eq!(t.network, "bitcoin");
        assert!(t.describe().contains("Transact Security Layer"));
    }
}
