//! Phase 0 Genesis Authority — sole source of truth for **tracking peers** and **banning peers**.
//!
//! # Security model (Phase 0)
//!
//! | Rule | How |
//! |------|-----|
//! | Only you ban / track | Secret key stays on genesis machine (`~/.grid/genesis/secret.key`) |
//! | Peers cannot forge bans | Ed25519 signatures over canonical snapshot (epoch + bans + tracked) |
//! | No remote ban API | HTTP only serves signed truth; ban/track is local CLI only |
//! | Replay resistance | Monotonic `epoch`; peers reject lower epochs |
//! | Peers enforce | Before accepting P2P hello, check ban list from verified snapshot |
//!
//! Bitcoin remains Transact Security Layer for value. Genesis is **peer-policy** authority only.

mod keys;
mod server;
pub mod store;
mod truth;

pub const CANONICAL_P2P_PEER: &str = "genesis.grid-compute.com:9900";
pub const CANONICAL_TRUTH_URL: &str = "http://genesis.grid-compute.com:9100";
pub const CANONICAL_LEADER_PUBKEY: &str =
    "bed7a2831dadf92016a480a6e2ae2ff77b745c6baac729387f4b1044bd158513";

pub use keys::{
    export_operational_key, export_pubkey_hex, generate_keypair, generate_protected, load_authority,
    load_keypair, load_protected, GenesisAuthority, GenesisKeys,
};
pub use server::run_genesis_server;
pub use store::{fetch_truth, GenesisStore};
pub use truth::{ban_count, is_banned, verify_truth, BanRecord, SignedTruth, TrackedPeer};
