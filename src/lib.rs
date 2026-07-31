//! GRID Phase 1 library — useful mining, PoR earn, Bitcoin as Transact Security Layer.
//!
//! ```text
//! Work (PoR jobs) → Utility (GRID earn ledger) → Exit (BTC TSL)
//! ```

pub mod address;
pub mod arc_pairing;
pub mod arc_protocol;
pub mod banner;
pub mod bench;
pub mod blockchain;
pub mod chain;
pub mod claim;
pub mod compute;
pub mod config;
pub mod consensus;
pub mod coord;
pub mod crypto;
pub mod earn;
pub mod ember;
pub mod engine;
pub mod executor;
pub mod wallet;

// Re-export executor constants for CLI payload defaults
pub use executor::DEFAULT_BLAKE3_ITERS;
pub mod genesis;
pub mod gp;
pub mod gui;
pub mod mesh_ping;
pub mod node;
pub mod p2p;
pub mod passkey;
pub mod por;
pub mod protocol;
pub mod register;
pub mod resources;
pub mod solana_wallet;
pub mod supply;
pub mod tsl;
pub mod version_gate;

pub use config::{NodeClass, NodeConfig};
pub use tsl::TransactSecurityLayer;
