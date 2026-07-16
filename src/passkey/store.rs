//! Passkey credential persistence (gitignored under ~/.grid/passkey/).

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use webauthn_rs::prelude::Passkey;

#[derive(Debug, Serialize, Deserialize)]
pub struct PasskeyStore {
    pub rp_id: String,
    pub user_name: String,
    pub passkey: Passkey,
    pub registered_at: String,
}

pub fn store_path(config_dir: &Path) -> PathBuf {
    config_dir.join("passkey").join("credential.json")
}

pub fn load(config_dir: &Path) -> Result<Option<PasskeyStore>> {
    let p = store_path(config_dir);
    if !p.exists() {
        return Ok(None);
    }
    let raw = std::fs::read_to_string(&p).with_context(|| format!("read {}", p.display()))?;
    Ok(Some(serde_json::from_str(&raw)?))
}

pub fn save(config_dir: &Path, store: &PasskeyStore) -> Result<()> {
    let p = store_path(config_dir);
    if let Some(parent) = p.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp = p.with_extension("json.tmp");
    std::fs::write(&tmp, serde_json::to_string_pretty(store)?)?;
    std::fs::rename(&tmp, &p)?;
    // best-effort 0600
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o600));
    }
    Ok(())
}

pub fn has_passkey(config_dir: &Path) -> bool {
    store_path(config_dir).exists()
}
