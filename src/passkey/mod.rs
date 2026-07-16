//! Operator vault — protect node crypto keys.
//!
//! ```text
//! grid auth                 # default = passkey
//! grid auth passkey
//! grid auth password
//! grid auth keyphrase       # 24-word phrase
//! grid auth combo           # password → passkey → keyphrase
//! grid auth master          # password + passkey + 24-word + master key (DESTROY)
//! grid auth nocrypt         # plaintext keys only
//! grid auth login | status | delete
//! ```
//!
//! Master mode: password + passkey + 24 words + master key. The master key is
//! shown once and **destroyed** on this node. Unlock needs every factor.
//!
//! Phase 2: ban policy may move to consensus; vault remains local key control.

mod ceremony;
mod store;
mod vault;

pub use ceremony::{register_passkey, require_passkey};
pub use vault::{
    auth_delete, auth_init, auth_login, auth_status, normalize_peer_target, require_unlocked,
    AuthMode, AuthStatus,
};
