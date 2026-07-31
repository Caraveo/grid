use std::path::PathBuf;
use std::time::Duration;

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};

use crate::blockchain::{
    block_hash, ChainReplica, Settlement, SettlementAllocation, SettlementNode,
};
use crate::chain::ChainState;

use super::GenesisKeys;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ChainStatus {
    chain_id: Option<String>,
    height: u64,
    tip_hash: Option<String>,
}

#[derive(Debug, Deserialize)]
struct SettlementList {
    settlements: Vec<PendingSettlement>,
}

#[derive(Debug, Clone, Deserialize)]
struct PendingSettlement {
    job_id: String,
    node_id: String,
    cluster_id: String,
    recipient: String,
    amount: f64,
    intent_commitment: String,
    result_commitment: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct BlockAck<'a> {
    chain_id: &'a str,
    height: u64,
    block_hash: &'a str,
    job_ids: &'a [String],
}

pub struct ProducerOptions {
    pub config_dir: PathBuf,
    pub coordinator: String,
    pub secret: String,
    pub poll: Duration,
    pub batch: usize,
    pub once: bool,
}

impl PendingSettlement {
    fn into_chain_settlement(self) -> Result<Settlement> {
        if self.job_id.is_empty()
            || self.recipient.is_empty()
            || self.amount <= 0.0
            || !self.amount.is_finite()
        {
            bail!("coordinator returned an invalid settlement");
        }
        // Pilot rewards created before node/cluster receipt binding have empty
        // identity fields. Preserve them deterministically using the reward
        // wallet so every historical settlement remains independently
        // replayable without inventing a mutable identity.
        let node_id = if self.node_id.trim().is_empty() {
            format!("legacy:{}", self.recipient)
        } else {
            self.node_id
        };
        let cluster_id = if self.cluster_id.trim().is_empty() {
            "legacy-pilot".to_string()
        } else {
            self.cluster_id
        };
        let settlement = Settlement {
            job_id: self.job_id,
            track: "mine".into(),
            intent_commitment: self.intent_commitment,
            result_commitment: self.result_commitment,
            verified: true,
            pool: self.amount,
            nodes: vec![SettlementNode {
                node_id: node_id.clone(),
                cluster_id,
                score: 1.0,
                // The current devnet queue emits one independently verified
                // miner per settlement; it therefore receives both the
                // proportional and inclusion portions of its fixed reward.
                class_s: true,
            }],
            allocations: vec![SettlementAllocation {
                node_id,
                amount: self.amount,
            }],
        };
        settlement.verify()?;
        Ok(settlement)
    }
}

pub async fn run_block_producer(keys: GenesisKeys, opts: ProducerOptions) -> Result<()> {
    let client = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(30))
        .build()?;
    let coordinator = opts.coordinator.trim_end_matches('/').to_string();
    let batch = opts.batch.clamp(1, 100);

    println!("GRID Genesis block producer");
    println!("  coordinator {coordinator}");
    println!("  batch       {batch}");
    println!("  poll        {}s", opts.poll.as_secs());

    loop {
        match produce_once(&client, &coordinator, &opts, &keys, batch).await {
            Ok(Some((height, hash, count))) => {
                println!(
                    "[chain] signed block height={height} hash={} settlements={count}",
                    &hash[..hash.len().min(16)]
                );
            }
            Ok(None) => {
                if opts.once {
                    println!("[chain] no pending settlements");
                }
            }
            Err(error) if !opts.once => {
                eprintln!("[chain] producer retry: {error:#}");
            }
            Err(error) => return Err(error),
        }
        if opts.once {
            return Ok(());
        }
        tokio::time::sleep(opts.poll).await;
    }
}

async fn produce_once(
    client: &reqwest::Client,
    coordinator: &str,
    opts: &ProducerOptions,
    keys: &GenesisKeys,
    batch: usize,
) -> Result<Option<(u64, String, usize)>> {
    let status = fetch_status(client, coordinator).await?;
    let mut replica = ChainReplica::load(&opts.config_dir)?
        .context("signed block genesis is missing from the Genesis host")?;
    replica.verify()?;
    if replica.leader_pubkey != keys.public_hex() {
        bail!("operational signer does not match the chain leader");
    }

    reconcile_blocks(client, coordinator, &opts.secret, &replica, &status).await?;

    let status = fetch_status(client, coordinator).await?;
    if status.height != replica.tip().height {
        bail!(
            "coordinator height {} does not match local height {} after reconciliation",
            status.height,
            replica.tip().height
        );
    }
    if let Some(remote_chain) = status.chain_id.as_deref() {
        if remote_chain != replica.chain_id {
            bail!("coordinator is anchored to a different chain");
        }
    }
    if let Some(remote_tip) = status.tip_hash.as_deref() {
        if remote_tip != block_hash(replica.tip())? {
            bail!("coordinator tip hash does not match the signed local chain");
        }
    }

    let response = client
        .get(format!("{coordinator}/v1/chain/settlements?limit={batch}"))
        .bearer_auth(&opts.secret)
        .send()
        .await
        .context("fetch pending chain settlements")?;
    if !response.status().is_success() {
        bail!("settlement endpoint returned HTTP {}", response.status());
    }
    let pending = response.json::<SettlementList>().await?.settlements;
    if pending.is_empty() {
        return Ok(None);
    }

    let mut state = ChainState::load(&opts.config_dir)?;
    let tx_start = state.txs.len();
    let mut settlements = Vec::with_capacity(pending.len());
    for row in pending {
        let settlement = row.into_chain_settlement()?;
        let allocation = settlement
            .allocations
            .first()
            .context("settlement allocation missing")?;
        let minted = state.mint_unclaimed(
            &allocation.node_id,
            &settlement.job_id,
            allocation.amount,
            &settlement.result_commitment,
        );
        if (minted - allocation.amount).abs() > 1e-8 {
            bail!(
                "emission controller refused settlement {} (requested {}, minted {})",
                settlement.job_id,
                allocation.amount,
                minted
            );
        }
        settlements.push(settlement);
    }
    let transactions = state.txs[tx_start..].to_vec();
    let block =
        replica.append_leader_block_with_settlements(keys, &state, transactions, settlements)?;
    let hash = block_hash(&block)?;

    replica.save(&opts.config_dir)?;
    state.save(&opts.config_dir)?;

    acknowledge_block(
        client,
        coordinator,
        &opts.secret,
        &replica.chain_id,
        &block,
        &hash,
    )
    .await?;
    Ok(Some((block.height, hash, block.settlements.len())))
}

async fn fetch_status(client: &reqwest::Client, coordinator: &str) -> Result<ChainStatus> {
    let response = client
        .get(format!("{coordinator}/v1/chain/status"))
        .send()
        .await
        .context("fetch coordinator chain status")?;
    if !response.status().is_success() {
        bail!("chain status returned HTTP {}", response.status());
    }
    Ok(response.json().await?)
}

async fn reconcile_blocks(
    client: &reqwest::Client,
    coordinator: &str,
    secret: &str,
    replica: &ChainReplica,
    status: &ChainStatus,
) -> Result<()> {
    if status.height > replica.tip().height {
        bail!(
            "coordinator height {} is ahead of local signed chain {}",
            status.height,
            replica.tip().height
        );
    }
    for block in replica
        .blocks
        .iter()
        .filter(|block| block.height > status.height)
    {
        if block.settlements.is_empty() {
            bail!(
                "cannot reconcile local block {} without settlement identifiers",
                block.height
            );
        }
        let hash = block_hash(block)?;
        acknowledge_block(client, coordinator, secret, &replica.chain_id, block, &hash).await?;
    }
    Ok(())
}

async fn acknowledge_block(
    client: &reqwest::Client,
    coordinator: &str,
    secret: &str,
    chain_id: &str,
    block: &crate::blockchain::Block,
    hash: &str,
) -> Result<()> {
    let job_ids = block
        .settlements
        .iter()
        .map(|settlement| settlement.job_id.clone())
        .collect::<Vec<_>>();
    let response = client
        .post(format!("{coordinator}/v1/chain/ack"))
        .bearer_auth(secret)
        .json(&BlockAck {
            chain_id,
            height: block.height,
            block_hash: hash,
            job_ids: &job_ids,
        })
        .send()
        .await
        .context("acknowledge signed block")?;
    if !response.status().is_success() {
        let status = response.status();
        let message = response.text().await.unwrap_or_default();
        bail!("block acknowledgement returned HTTP {status}: {message}");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn coordinator_row_becomes_replayable_settlement() {
        let commitment = "a".repeat(64);
        let row = PendingSettlement {
            job_id: "job-1".into(),
            node_id: "node-1".into(),
            cluster_id: "cluster-1".into(),
            recipient: "11111111111111111111111111111111".into(),
            amount: 100.0,
            intent_commitment: commitment.clone(),
            result_commitment: commitment,
        };
        let settlement = row.into_chain_settlement().unwrap();
        assert_eq!(settlement.allocations[0].amount, 100.0);
        settlement.verify().unwrap();
    }

    #[test]
    fn legacy_receipt_uses_stable_reward_wallet_identity() {
        let commitment = "b".repeat(64);
        let row = PendingSettlement {
            job_id: "job-legacy".into(),
            node_id: String::new(),
            cluster_id: String::new(),
            recipient: "reward-wallet".into(),
            amount: 100.0,
            intent_commitment: commitment.clone(),
            result_commitment: commitment,
        };
        let settlement = row.into_chain_settlement().unwrap();
        assert_eq!(settlement.nodes[0].node_id, "legacy:reward-wallet");
        assert_eq!(settlement.nodes[0].cluster_id, "legacy-pilot");
        settlement.verify().unwrap();
    }
}
