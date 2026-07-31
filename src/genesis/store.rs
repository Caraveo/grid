//! Persistent genesis store (local only — ban/track requires secret key).

use anyhow::{Context, Result};
use chrono::Utc;
use std::path::{Path, PathBuf};
use uuid::Uuid;

use super::keys::{load_keypair, GenesisKeys};
use super::truth::{sign_truth, BanRecord, SignedTruth, TrackedPeer, TruthBody};

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Default)]
struct RawStore {
    epoch: u64,
    tracked: Vec<TrackedPeer>,
    banned: Vec<BanRecord>,
}

pub struct GenesisStore {
    path: PathBuf,
    raw: RawStore,
    keys: GenesisKeys,
}

impl GenesisStore {
    pub fn open(config_dir: &Path) -> Result<Self> {
        Self::open_with_keys(config_dir, load_keypair(config_dir)?)
    }

    /// Open with a protected authority key that was decrypted after vault auth.
    pub fn open_with_keys(config_dir: &Path, keys: GenesisKeys) -> Result<Self> {
        let path = config_dir.join("genesis").join("truth.json");
        let raw = if path.exists() {
            serde_json::from_str(&std::fs::read_to_string(&path)?)?
        } else {
            RawStore::default()
        };
        Ok(Self { path, raw, keys })
    }

    pub fn keys(&self) -> &GenesisKeys {
        &self.keys
    }

    fn save(&self) -> Result<()> {
        if let Some(p) = self.path.parent() {
            std::fs::create_dir_all(p)?;
        }
        let tmp = self.path.with_extension("json.tmp");
        std::fs::write(&tmp, serde_json::to_string_pretty(&self.raw)?)?;
        std::fs::rename(&tmp, &self.path)?;
        Ok(())
    }

    fn bump_epoch(&mut self) {
        self.raw.epoch = self.raw.epoch.saturating_add(1);
    }

    pub fn snapshot(&self) -> Result<SignedTruth> {
        let body = TruthBody {
            epoch: self.raw.epoch,
            issued_at: Utc::now().to_rfc3339(),
            genesis_pubkey: String::new(),
            minimum_cli_version: crate::version_gate::configured_minimum(),
            tracked: self.raw.tracked.clone(),
            banned: self.raw.banned.clone(),
        };
        sign_truth(&self.keys, body)
    }

    /// Track a peer (genesis operator only).
    pub fn track(&mut self, peer_id: &str, name: &str, listen: &str, class: &str) -> Result<()> {
        if self.raw.banned.iter().any(|b| b.peer_id == peer_id) {
            anyhow::bail!("peer {peer_id} is banned — unban before tracking");
        }
        self.raw.tracked.retain(|p| p.peer_id != peer_id);
        self.raw.tracked.push(TrackedPeer {
            peer_id: peer_id.into(),
            name: name.into(),
            listen: listen.into(),
            class: class.into(),
            tracked_at: Utc::now().to_rfc3339(),
        });
        self.bump_epoch();
        self.save()?;
        Ok(())
    }

    pub fn untrack(&mut self, peer_id: &str) -> Result<bool> {
        let before = self.raw.tracked.len();
        self.raw.tracked.retain(|p| p.peer_id != peer_id);
        if self.raw.tracked.len() == before {
            return Ok(false);
        }
        self.bump_epoch();
        self.save()?;
        Ok(true)
    }

    /// Ban a peer — sole authority action. Removes from tracked list.
    pub fn ban(&mut self, peer_id: &str, reason: &str) -> Result<BanRecord> {
        if reason.trim().is_empty() {
            anyhow::bail!("ban reason required");
        }
        if peer_id.trim().is_empty() {
            anyhow::bail!("peer_id required");
        }
        // replace existing ban
        self.raw.banned.retain(|b| b.peer_id != peer_id);
        self.raw.tracked.retain(|p| p.peer_id != peer_id);
        let rec = BanRecord {
            peer_id: peer_id.into(),
            reason: reason.into(),
            banned_at: Utc::now().to_rfc3339(),
            ban_id: Uuid::new_v4().to_string(),
        };
        self.raw.banned.push(rec.clone());
        self.bump_epoch();
        self.save()?;
        Ok(rec)
    }

    pub fn unban(&mut self, peer_id: &str) -> Result<bool> {
        let before = self.raw.banned.len();
        self.raw.banned.retain(|b| b.peer_id != peer_id);
        if self.raw.banned.len() == before {
            return Ok(false);
        }
        self.bump_epoch();
        self.save()?;
        Ok(true)
    }

    pub fn list_tracked(&self) -> &[TrackedPeer] {
        &self.raw.tracked
    }

    pub fn list_banned(&self) -> &[BanRecord] {
        &self.raw.banned
    }

    pub fn epoch(&self) -> u64 {
        self.raw.epoch
    }
}

/// Fetch and verify truth from a genesis HTTP endpoint.
pub async fn fetch_truth(url: &str, expected_pubkey: Option<&str>) -> Result<SignedTruth> {
    let base = url.trim_end_matches('/');
    let client = reqwest::Client::new();
    let res = client
        .get(format!("{base}/v1/truth"))
        .send()
        .await
        .context("fetch genesis truth")?;
    if !res.status().is_success() {
        anyhow::bail!("genesis truth HTTP {}", res.status());
    }
    let truth: SignedTruth = res.json().await?;
    super::truth::verify_truth(&truth, expected_pubkey)?;
    Ok(truth)
}
