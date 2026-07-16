//! GRID Phase 1 library — useful mining, PoR earn, Bitcoin as Transact Security Layer.
//!
//! ```text
//! Work (PoR jobs) → Utility (GRID earn ledger) → Exit (BTC TSL)
//! ```

pub mod banner;
pub mod bench;
pub mod config;
pub mod coord;
pub mod crypto;
pub mod earn;
pub mod executor;
pub mod genesis;
pub mod mesh_ping;
pub mod node;
pub mod p2p;
pub mod passkey;
pub mod por;
pub mod protocol;
pub mod resources;
pub mod tsl;

pub use config::{NodeClass, NodeConfig};
pub use tsl::TransactSecurityLayer;
