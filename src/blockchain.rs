//! Signed leader-chain blocks and deterministic replica validation.
//!
//! The leader is permitted to propose blocks; every peer independently verifies
//! the signature, height, parent hash, and state root before persisting a copy.

use anyhow::{bail, Result};
use chrono::Utc;
use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use uuid::Uuid;

use crate::chain::{ChainState, ChainTx, MAX_SUPPLY};
use crate::crypto::blake3_hex;
use crate::genesis::GenesisKeys;
use crate::por::{allocate_inclusion, allocate_proportional, split_emission, NodeScore};

const BLOCKS_FILE: &str = "blocks.json";
const DOMAIN: &str = "GRID-BLOCK-v1";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Block {
    pub version: u32,
    pub chain_id: String,
    pub height: u64,
    pub previous_hash: String,
    pub timestamp: String,
    pub leader_pubkey: String,
    pub state_root: String,
    pub transactions: Vec<ChainTx>,
    #[serde(default)]
    pub settlements: Vec<Settlement>,
    pub signature: String,
}

/// Complete, replayable reward calculation committed into a signed block.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Settlement {
    pub job_id: String,
    pub track: String,
    /// Hash of the launcher-signed job specification/intent.
    pub intent_commitment: String,
    /// Hash of the independently verified output.
    pub result_commitment: String,
    pub verified: bool,
    pub pool: f64,
    pub nodes: Vec<SettlementNode>,
    pub allocations: Vec<SettlementAllocation>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SettlementNode {
    pub node_id: String,
    pub cluster_id: String,
    pub score: f64,
    pub class_s: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SettlementAllocation {
    pub node_id: String,
    pub amount: f64,
}

impl Settlement {
    pub fn from_scores(
        job_id: String,
        track: String,
        intent_commitment: String,
        result_commitment: String,
        pool: f64,
        scores: &[NodeScore],
    ) -> Self {
        let (prop, inclusion) = split_emission(pool);
        let mut allocations = std::collections::BTreeMap::<String, f64>::new();
        for (id, amount) in allocate_proportional(scores, prop) {
            *allocations.entry(id).or_default() += amount;
        }
        for (id, amount) in allocate_inclusion(scores, inclusion) {
            *allocations.entry(id).or_default() += amount;
        }
        Self {
            job_id,
            track,
            intent_commitment,
            result_commitment,
            verified: true,
            pool,
            nodes: scores
                .iter()
                .map(|n| SettlementNode {
                    node_id: n.node_id.clone(),
                    cluster_id: n.cluster_id.clone(),
                    score: n.score,
                    class_s: n.class_s,
                })
                .collect(),
            allocations: allocations
                .into_iter()
                .map(|(node_id, amount)| SettlementAllocation { node_id, amount })
                .collect(),
        }
    }

    /// Replica-side deterministic replay. No coordinator amount is trusted.
    pub fn verify(&self) -> Result<()> {
        if !self.verified
            || self.intent_commitment.len() != 64
            || self.result_commitment.len() != 64
        {
            bail!("unverified or intent-mismatched settlement cannot allocate rewards");
        }
        if self.pool <= 0.0 || !self.pool.is_finite() || self.nodes.is_empty() {
            bail!("invalid settlement inputs");
        }
        let scores: Vec<NodeScore> = self
            .nodes
            .iter()
            .map(|n| NodeScore {
                node_id: n.node_id.clone(),
                cluster_id: n.cluster_id.clone(),
                score: n.score,
                class_s: n.class_s,
            })
            .collect();
        let (prop, inclusion) = split_emission(self.pool);
        let mut expected = std::collections::BTreeMap::<String, f64>::new();
        for (id, amount) in allocate_proportional(&scores, prop) {
            *expected.entry(id).or_default() += amount;
        }
        for (id, amount) in allocate_inclusion(&scores, inclusion) {
            *expected.entry(id).or_default() += amount;
        }
        let mut supplied = std::collections::BTreeMap::<String, f64>::new();
        for a in &self.allocations {
            if a.amount < 0.0 || !a.amount.is_finite() {
                bail!("invalid allocation");
            }
            *supplied.entry(a.node_id.clone()).or_default() += a.amount;
        }
        if expected.len() != supplied.len() {
            bail!("settlement allocation members mismatch");
        }
        for (id, amount) in expected {
            if (supplied.get(&id).copied().unwrap_or(-1.0) - amount).abs() > 1e-8 {
                bail!("settlement allocation mismatch for {id}");
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChainReplica {
    pub chain_id: String,
    pub leader_pubkey: String,
    pub max_supply: f64,
    pub recovery_pubkeys: Vec<String>,
    pub blocks: Vec<Block>,
}

impl ChainReplica {
    pub fn path_in(dir: &Path) -> PathBuf {
        dir.join("chain").join(BLOCKS_FILE)
    }

    pub fn load(dir: &Path) -> Result<Option<Self>> {
        let p = Self::path_in(dir);
        if !p.exists() {
            return Ok(None);
        }
        Ok(Some(serde_json::from_str(&std::fs::read_to_string(p)?)?))
    }

    pub fn save(&self, dir: &Path) -> Result<()> {
        self.verify()?;
        let p = Self::path_in(dir);
        std::fs::create_dir_all(p.parent().expect("chain parent"))?;
        let tmp = p.with_extension("json.tmp");
        std::fs::write(&tmp, serde_json::to_string_pretty(self)?)?;
        std::fs::rename(tmp, p)?;
        Ok(())
    }

    pub fn create_genesis(keys: &GenesisKeys, recovery_pubkeys: Vec<String>) -> Result<Self> {
        let chain_id = format!("grid-{}", Uuid::new_v4());
        let state = ChainState::default();
        let mut replica = Self {
            chain_id: chain_id.clone(),
            leader_pubkey: keys.public_hex(),
            max_supply: MAX_SUPPLY,
            recovery_pubkeys,
            blocks: vec![],
        };
        let genesis = signed_block(keys, &chain_id, 0, "", &state, vec![], vec![])?;
        replica.blocks.push(genesis);
        replica.verify()?;
        Ok(replica)
    }

    pub fn tip(&self) -> &Block {
        self.blocks.last().expect("verified nonempty chain")
    }

    pub fn append_leader_block(
        &mut self,
        keys: &GenesisKeys,
        state: &ChainState,
        txs: Vec<ChainTx>,
    ) -> Result<Block> {
        self.append_leader_block_with_settlements(keys, state, txs, vec![])
    }

    pub fn append_leader_block_with_settlements(
        &mut self,
        keys: &GenesisKeys,
        state: &ChainState,
        txs: Vec<ChainTx>,
        settlements: Vec<Settlement>,
    ) -> Result<Block> {
        if keys.public_hex() != self.leader_pubkey {
            bail!("only configured leader may propose blocks");
        }
        let b = signed_block(
            keys,
            &self.chain_id,
            self.tip().height + 1,
            &block_hash(self.tip())?,
            state,
            txs,
            settlements,
        )?;
        self.apply_replica_block(b.clone())?;
        Ok(b)
    }

    pub fn apply_replica_block(&mut self, block: Block) -> Result<()> {
        if block.chain_id != self.chain_id || block.leader_pubkey != self.leader_pubkey {
            bail!("wrong chain trust anchor");
        }
        let tip = self.tip();
        if block.height != tip.height + 1 || block.previous_hash != block_hash(tip)? {
            bail!("block does not extend current tip");
        }
        verify_block(&block)?;
        self.blocks.push(block);
        Ok(())
    }

    pub fn verify(&self) -> Result<()> {
        if self.blocks.is_empty() {
            bail!("missing genesis block");
        }
        let mut expected_prev = String::new();
        for (i, b) in self.blocks.iter().enumerate() {
            if b.chain_id != self.chain_id
                || b.leader_pubkey != self.leader_pubkey
                || b.height != i as u64
                || b.previous_hash != expected_prev
            {
                bail!("invalid block linkage at height {i}");
            }
            verify_block(b)?;
            expected_prev = block_hash(b)?;
        }
        Ok(())
    }
}

pub fn state_root(state: &ChainState) -> Result<String> {
    Ok(blake3_hex(&serde_json::to_vec(state)?))
}

pub fn block_hash(block: &Block) -> Result<String> {
    Ok(blake3_hex(&serde_json::to_vec(block)?))
}

fn signing_bytes(block: &Block) -> Result<Vec<u8>> {
    let mut b = block.clone();
    b.signature.clear();
    let mut out = DOMAIN.as_bytes().to_vec();
    out.push(b'\n');
    out.extend(serde_json::to_vec(&b)?);
    Ok(out)
}

fn signed_block(
    keys: &GenesisKeys,
    chain_id: &str,
    height: u64,
    previous_hash: &str,
    state: &ChainState,
    transactions: Vec<ChainTx>,
    settlements: Vec<Settlement>,
) -> Result<Block> {
    let mut b = Block {
        version: 1,
        chain_id: chain_id.into(),
        height,
        previous_hash: previous_hash.into(),
        timestamp: Utc::now().to_rfc3339(),
        leader_pubkey: keys.public_hex(),
        state_root: state_root(state)?,
        transactions,
        settlements,
        signature: String::new(),
    };
    b.signature = keys.sign(&signing_bytes(&b)?);
    Ok(b)
}

pub fn verify_block(block: &Block) -> Result<()> {
    let key: [u8; 32] = hex::decode(&block.leader_pubkey)?
        .as_slice()
        .try_into()
        .map_err(|_| anyhow::anyhow!("leader key length"))?;
    let sig: [u8; 64] = hex::decode(&block.signature)?
        .as_slice()
        .try_into()
        .map_err(|_| anyhow::anyhow!("block signature length"))?;
    VerifyingKey::from_bytes(&key)?
        .verify(&signing_bytes(block)?, &Signature::from_bytes(&sig))
        .map_err(|_| anyhow::anyhow!("invalid block signature"))?;
    for settlement in &block.settlements {
        settlement.verify()?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::genesis::generate_keypair;
    use tempfile::tempdir;
    #[test]
    fn signed_chain_rejects_tamper() {
        let d = tempdir().unwrap();
        let k = generate_keypair(d.path()).unwrap();
        let mut r = ChainReplica::create_genesis(&k, vec![]).unwrap();
        let s = ChainState::default();
        r.append_leader_block(&k, &s, vec![]).unwrap();
        assert!(r.verify().is_ok());
        r.blocks[1].height = 7;
        assert!(r.verify().is_err());
    }
}
