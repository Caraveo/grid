//! Signed genesis truth snapshots.

use anyhow::Result;
use serde::{Deserialize, Serialize};

use super::keys::{parse_pubkey, verify_sig, GenesisKeys};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TrackedPeer {
    pub peer_id: String,
    pub name: String,
    pub listen: String,
    pub class: String,
    pub tracked_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BanRecord {
    pub peer_id: String,
    pub reason: String,
    pub banned_at: String,
    /// Unique ban id (uuid) for audit
    pub ban_id: String,
}

/// Payload that is signed (never includes signature field).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TruthBody {
    /// Monotonic epoch — peers reject snapshots with epoch < last seen.
    pub epoch: u64,
    pub issued_at: String,
    pub genesis_pubkey: String,
    pub tracked: Vec<TrackedPeer>,
    pub banned: Vec<BanRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignedTruth {
    #[serde(flatten)]
    pub body: TruthBody,
    pub signature: String,
}

/// Canonical bytes for signing: blake3 of compact JSON with sorted keys via serde_json Value sort...
/// We use a fixed field order by serializing TruthBody with serde_json::to_vec (field order stable in struct).
pub fn canonical_bytes(body: &TruthBody) -> Result<Vec<u8>> {
    // Deterministic: serde field order on TruthBody + compact JSON
    let json = serde_json::to_vec(body)?;
    Ok(blake3::hash(&json).as_bytes().to_vec())
}

pub fn sign_truth(keys: &GenesisKeys, mut body: TruthBody) -> Result<SignedTruth> {
    body.genesis_pubkey = keys.public_hex();
    let msg = canonical_bytes(&body)?;
    let signature = keys.sign(&msg);
    Ok(SignedTruth { body, signature })
}

pub fn verify_truth(truth: &SignedTruth, expected_pubkey_hex: Option<&str>) -> Result<()> {
    if let Some(exp) = expected_pubkey_hex {
        if exp.trim() != truth.body.genesis_pubkey.trim() {
            anyhow::bail!("genesis pubkey mismatch with configured trust anchor");
        }
    }
    let vk = parse_pubkey(&truth.body.genesis_pubkey)?;
    let msg = canonical_bytes(&truth.body)?;
    verify_sig(&vk, &msg, &truth.signature)?;
    Ok(())
}

/// True if `peer_id` appears on the signed ban list (one-shot check).
/// Live mesh uses an in-memory map after truth refresh; this helper is for
/// operators and tests.
pub fn is_banned(truth: &SignedTruth, peer_id: &str) -> bool {
    truth
        .body
        .banned
        .iter()
        .any(|b| b.peer_id == peer_id)
}

/// Count of currently banned peer ids in a verified snapshot.
pub fn ban_count(truth: &SignedTruth) -> usize {
    truth.body.banned.len()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::genesis::keys::generate_keypair;
    use tempfile::tempdir;

    #[test]
    fn sign_and_verify() {
        let dir = tempdir().unwrap();
        let keys = generate_keypair(dir.path()).unwrap();
        let body = TruthBody {
            epoch: 1,
            issued_at: "t".into(),
            genesis_pubkey: String::new(),
            tracked: vec![],
            banned: vec![BanRecord {
                peer_id: "evil".into(),
                reason: "spam".into(),
                banned_at: "t".into(),
                ban_id: "b1".into(),
            }],
        };
        let signed = sign_truth(&keys, body).unwrap();
        verify_truth(&signed, Some(&keys.public_hex())).unwrap();
        assert!(is_banned(&signed, "evil"));
        assert!(!is_banned(&signed, "good"));
    }

    #[test]
    fn reject_tamper() {
        let dir = tempdir().unwrap();
        let keys = generate_keypair(dir.path()).unwrap();
        let body = TruthBody {
            epoch: 1,
            issued_at: "t".into(),
            genesis_pubkey: String::new(),
            tracked: vec![],
            banned: vec![],
        };
        let mut signed = sign_truth(&keys, body).unwrap();
        signed.body.banned.push(BanRecord {
            peer_id: "forged".into(),
            reason: "no".into(),
            banned_at: "t".into(),
            ban_id: "x".into(),
        });
        assert!(verify_truth(&signed, None).is_err());
    }
}
