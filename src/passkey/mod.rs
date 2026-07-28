//! Operator vault — protect node crypto keys.
//!
//! ```text
//! grid auth                 # default = passkey
//! grid auth passkey
//! grid auth password
//! grid auth keyphrase       # 24-word phrase
//! grid auth combo           # password → passkey → keyphrase
//! grid auth nocrypt         # plaintext keys only
//! grid auth login | status | delete
//! ```
//!
//! Legacy master vaults (four-factor + off-node key) can still `grid auth login`.
//! New master / DESTROY setup is removed — not required for genesis.
//!
//! Phase 2: ban policy may move to consensus; vault remains local key control.

mod ceremony;
mod store;
mod vault;

pub use ceremony::{register_passkey, require_passkey};
pub use vault::{
    auth_delete, auth_init, auth_init_combo_gui, auth_init_keyphrase_gui, auth_init_password_gui,
    auth_login, auth_status, auth_unlock_gui, decrypt_with_vault, encrypt_with_vault,
    load_operator_signing_key, normalize_peer_target, operator_pubkey_hex, p2p_noise_static_key,
    require_identity, require_unlocked, sign_operator, verify_operator_sig, AuthMode, AuthStatus,
};
