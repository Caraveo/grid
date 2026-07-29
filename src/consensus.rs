//! Validator quorum primitives and the production-mainnet launch gate.
//!
//! These types deliberately do not pretend the current leader chain is BFT.
//! A quorum certificate is valid only when more than two thirds of a pinned,
//! independent validator set sign the exact same proposal hash.

use std::collections::BTreeSet;
use std::path::Path;

use anyhow::{bail, Context, Result};
use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use serde::{Deserialize, Serialize};

use crate::blockchain::ChainReplica;
use crate::chain::{COMPUTE_ALLOCATION, MAX_SUPPLY};

const VOTE_DOMAIN: &str = "GRID-VALIDATOR-VOTE-v1";
pub const MIN_PRODUCTION_VALIDATORS: usize = 4;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ValidatorSet {
    pub epoch: u64,
    pub validators: Vec<String>,
}

impl ValidatorSet {
    pub fn verify(&self) -> Result<()> {
        if self.epoch == 0 {
            bail!("validator epoch must be positive");
        }
        if self.validators.is_empty() || self.validators.len() > 100 {
            bail!("validator set size must be 1..=100");
        }
        let mut unique = BTreeSet::new();
        for key in &self.validators {
            parse_pubkey(key)?;
            if !unique.insert(key.to_ascii_lowercase()) {
                bail!("duplicate validator public key");
            }
        }
        Ok(())
    }

    pub fn quorum(&self) -> usize {
        (self.validators.len() * 2) / 3 + 1
    }

    pub fn is_production_sized(&self) -> bool {
        self.validators.len() >= MIN_PRODUCTION_VALIDATORS
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ValidatorVote {
    pub chain_id: String,
    pub height: u64,
    pub proposal_hash: String,
    pub epoch: u64,
    pub validator_pubkey: String,
    pub signature: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct QuorumCertificate {
    pub chain_id: String,
    pub height: u64,
    pub proposal_hash: String,
    pub epoch: u64,
    pub votes: Vec<ValidatorVote>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct UnsignedVote<'a> {
    chain_id: &'a str,
    height: u64,
    proposal_hash: &'a str,
    epoch: u64,
    validator_pubkey: &'a str,
}

fn validate_hash(value: &str) -> Result<()> {
    if value.len() != 64 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        bail!("proposal hash must be 32-byte hex");
    }
    Ok(())
}

fn parse_pubkey(value: &str) -> Result<VerifyingKey> {
    let raw = hex::decode(value).context("decode validator public key")?;
    let bytes: [u8; 32] = raw
        .as_slice()
        .try_into()
        .map_err(|_| anyhow::anyhow!("validator public key must be 32 bytes"))?;
    Ok(VerifyingKey::from_bytes(&bytes)?)
}

fn vote_bytes(vote: &ValidatorVote) -> Result<Vec<u8>> {
    let unsigned = UnsignedVote {
        chain_id: &vote.chain_id,
        height: vote.height,
        proposal_hash: &vote.proposal_hash,
        epoch: vote.epoch,
        validator_pubkey: &vote.validator_pubkey,
    };
    let mut bytes = VOTE_DOMAIN.as_bytes().to_vec();
    bytes.push(b'\n');
    bytes.extend(serde_json::to_vec(&unsigned)?);
    Ok(bytes)
}

impl ValidatorVote {
    pub fn sign(
        signing: &SigningKey,
        chain_id: &str,
        height: u64,
        proposal_hash: &str,
        epoch: u64,
    ) -> Result<Self> {
        if chain_id.trim().is_empty() || chain_id.len() > 128 {
            bail!("invalid chain id");
        }
        validate_hash(proposal_hash)?;
        let mut vote = Self {
            chain_id: chain_id.into(),
            height,
            proposal_hash: proposal_hash.to_ascii_lowercase(),
            epoch,
            validator_pubkey: hex::encode(signing.verifying_key().as_bytes()),
            signature: String::new(),
        };
        vote.signature = hex::encode(signing.sign(&vote_bytes(&vote)?).to_bytes());
        Ok(vote)
    }

    pub fn verify(&self) -> Result<()> {
        validate_hash(&self.proposal_hash)?;
        let signature_raw = hex::decode(&self.signature).context("decode validator signature")?;
        let signature: [u8; 64] = signature_raw
            .as_slice()
            .try_into()
            .map_err(|_| anyhow::anyhow!("validator signature must be 64 bytes"))?;
        parse_pubkey(&self.validator_pubkey)?
            .verify(&vote_bytes(self)?, &Signature::from_bytes(&signature))
            .map_err(|_| anyhow::anyhow!("invalid validator signature"))
    }
}

impl QuorumCertificate {
    pub fn verify(&self, validators: &ValidatorSet) -> Result<()> {
        validators.verify()?;
        if self.epoch != validators.epoch {
            bail!("certificate validator epoch mismatch");
        }
        validate_hash(&self.proposal_hash)?;
        let members: BTreeSet<String> = validators
            .validators
            .iter()
            .map(|key| key.to_ascii_lowercase())
            .collect();
        let mut signers = BTreeSet::new();
        for vote in &self.votes {
            if vote.chain_id != self.chain_id
                || vote.height != self.height
                || vote.proposal_hash != self.proposal_hash
                || vote.epoch != self.epoch
            {
                bail!("vote does not match certificate proposal");
            }
            vote.verify()?;
            let signer = vote.validator_pubkey.to_ascii_lowercase();
            if !members.contains(&signer) {
                bail!("vote signer is not in validator set");
            }
            if !signers.insert(signer) {
                bail!("duplicate validator vote");
            }
        }
        if signers.len() < validators.quorum() {
            bail!(
                "insufficient quorum: got {}, require {}",
                signers.len(),
                validators.quorum()
            );
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReadinessCheck {
    pub name: String,
    pub ok: bool,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MainnetReadiness {
    pub ready: bool,
    pub network: String,
    pub height: Option<u64>,
    pub checks: Vec<ReadinessCheck>,
    pub blockers: Vec<String>,
}

fn check(name: &str, ok: bool, detail: impl Into<String>) -> ReadinessCheck {
    ReadinessCheck {
        name: name.into(),
        ok,
        detail: detail.into(),
    }
}

pub fn readiness(config_dir: &Path) -> MainnetReadiness {
    let replica = ChainReplica::load(config_dir).ok().flatten();
    let chain_valid = replica.as_ref().is_some_and(|chain| chain.verify().is_ok());
    let total_supply_ok = replica
        .as_ref()
        .is_some_and(|chain| (chain.max_supply - MAX_SUPPLY).abs() < f64::EPSILON);
    let recovery_ok = replica
        .as_ref()
        .is_some_and(|chain| chain.recovery_pubkeys.len() >= 2);
    let heartbeat_key = config_dir.join("keys").join("mesh-heartbeat.key");
    let heartbeat_ok = heartbeat_key.exists()
        && std::fs::metadata(&heartbeat_key)
            .map(|metadata| metadata.len() == 32)
            .unwrap_or(false);

    // This remains deliberately false until finalized blocks carry and enforce
    // a verified QuorumCertificate from an epoch-pinned validator set.
    let quorum_finality_enforced = false;
    let split_storage = ChainReplica::uses_split_storage(config_dir);
    let checks = vec![
        check(
            "chain.replica",
            chain_valid,
            if chain_valid {
                "signed chain linkage and block signatures verify"
            } else {
                "no valid replicated chain found"
            },
        ),
        check(
            "supply.total_cap",
            total_supply_ok,
            replica
                .as_ref()
                .map(|chain| {
                    format!(
                        "total cap is {:.0} GRID; protocol policy is {:.0}",
                        chain.max_supply, MAX_SUPPLY
                    )
                })
                .unwrap_or_else(|| "chain unavailable".into()),
        ),
        check(
            "supply.compute_allocation",
            (COMPUTE_ALLOCATION - 5_000_000_000.0).abs() < f64::EPSILON,
            format!(
                "verified-work minting stops at {:.0} GRID independently of treasury",
                COMPUTE_ALLOCATION
            ),
        ),
        check(
            "supply.treasury_allocation",
            false,
            "BLOCKER: the separate 5B treasury allocation, vesting schedules, and multisig controls are not represented in consensus state",
        ),
        check(
            "recovery.keys",
            recovery_ok,
            "at least two offline recovery public keys are required",
        ),
        check(
            "node.heartbeat",
            heartbeat_ok,
            if heartbeat_ok {
                "dedicated Ed25519 heartbeat identity exists"
            } else {
                "signed heartbeat key is created on the first location-enabled node pulse"
            },
        ),
        check(
            "consensus.validator_quorum",
            quorum_finality_enforced,
            "BLOCKER: blocks are still finalized by one Genesis leader; require 3-of-4 validator certificates",
        ),
        check(
            "consensus.independent_validators",
            false,
            "BLOCKER: four independently operated validator nodes are not configured",
        ),
        check(
            "storage.append_only_blocks",
            split_storage,
            if split_storage {
                "immutable per-height block files are active"
            } else {
                "BLOCKER: migrate blocks.json with `grid mainnet --migrate-storage`"
            },
        ),
        check(
            "security.external_audit",
            false,
            "BLOCKER: consensus, key management, and economic invariants need an independent audit",
        ),
    ];
    let blockers = checks
        .iter()
        .filter(|finding| !finding.ok)
        .map(|finding| finding.detail.clone())
        .collect::<Vec<_>>();
    MainnetReadiness {
        ready: blockers.is_empty(),
        network: "production-pilot".into(),
        height: replica.as_ref().map(|chain| chain.tip().height),
        checks,
        blockers,
    }
}

pub fn print_readiness(report: &MainnetReadiness, json: bool) -> Result<()> {
    if json {
        println!("{}", serde_json::to_string_pretty(report)?);
        return Ok(());
    }
    println!("GRID decentralized mainnet launch gate");
    println!(
        "  result   {}",
        if report.ready {
            "READY"
        } else {
            "NOT READY — production pilot only"
        }
    );
    println!(
        "  height   {}",
        report
            .height
            .map(|height| height.to_string())
            .unwrap_or_else(|| "—".into())
    );
    println!();
    for finding in &report.checks {
        println!(
            "  [{:4}] {:32} {}",
            if finding.ok { "OK" } else { "FAIL" },
            finding.name,
            finding.detail
        );
    }
    println!();
    println!("A failed gate is intentional: GRID must not call a single-signer");
    println!("pilot a decentralized mainnet.");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::rngs::OsRng;
    use tempfile::tempdir;

    #[test]
    fn three_of_four_votes_finalize_and_tampering_fails() {
        let keys = (0..4)
            .map(|_| SigningKey::generate(&mut OsRng))
            .collect::<Vec<_>>();
        let validators = ValidatorSet {
            epoch: 1,
            validators: keys
                .iter()
                .map(|key| hex::encode(key.verifying_key().as_bytes()))
                .collect(),
        };
        assert_eq!(validators.quorum(), 3);
        let hash = "ab".repeat(32);
        let votes = keys[..3]
            .iter()
            .map(|key| ValidatorVote::sign(key, "grid-test", 7, &hash, 1).unwrap())
            .collect();
        let certificate = QuorumCertificate {
            chain_id: "grid-test".into(),
            height: 7,
            proposal_hash: hash,
            epoch: 1,
            votes,
        };
        certificate.verify(&validators).unwrap();

        let mut tampered = certificate;
        tampered.height = 8;
        assert!(tampered.verify(&validators).is_err());
    }

    #[test]
    fn two_of_four_is_not_quorum_and_gate_stays_closed() {
        let keys = (0..4)
            .map(|_| SigningKey::generate(&mut OsRng))
            .collect::<Vec<_>>();
        let validators = ValidatorSet {
            epoch: 2,
            validators: keys
                .iter()
                .map(|key| hex::encode(key.verifying_key().as_bytes()))
                .collect(),
        };
        let hash = "cd".repeat(32);
        let certificate = QuorumCertificate {
            chain_id: "grid-test".into(),
            height: 9,
            proposal_hash: hash.clone(),
            epoch: 2,
            votes: keys[..2]
                .iter()
                .map(|key| ValidatorVote::sign(key, "grid-test", 9, &hash, 2).unwrap())
                .collect(),
        };
        assert!(certificate.verify(&validators).is_err());

        let dir = tempdir().unwrap();
        let report = readiness(dir.path());
        assert!(!report.ready);
        assert!(report
            .checks
            .iter()
            .any(|finding| finding.name == "consensus.validator_quorum" && !finding.ok));
    }
}
