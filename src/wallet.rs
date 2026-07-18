//! GRID wallet — thin client over the **on-chain** ledger.
//!
//! Burn is **not** implemented here. The blockchain (`chain.rs`) applies
//! protocol burns. Wallet only: init address, claim, send, receive, status.

use anyhow::{bail, Context, Result};
use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

use crate::address::{encode_payment_hex, is_valid_address, normalize_address};
use crate::chain::{
    print_chain_banner, print_supply, ChainState, ChainTx, BURN_DEADLINE_DAYS, MAX_SUPPLY,
};
use crate::config::NodeConfig;
use crate::passkey::{
    load_operator_signing_key, operator_pubkey_hex, require_unlocked, sign_operator,
};

/// Re-export for CLI messaging.
pub const CLAIM_DEADLINE_DAYS: i64 = BURN_DEADLINE_DAYS;

const WALLET_META: &str = "wallet.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WalletMeta {
    pub version: u32,
    pub address: String,
    pub pubkey_hex: String,
    pub created_at: String,
}

fn meta_path(config_dir: &Path) -> PathBuf {
    config_dir.join(WALLET_META)
}

impl WalletMeta {
    pub fn load(config_dir: &Path) -> Result<Self> {
        let p = meta_path(config_dir);
        if !p.exists() {
            bail!("no wallet — run: grid wallet init");
        }
        Ok(serde_json::from_str(&std::fs::read_to_string(&p)?)?)
    }

    pub fn save(&self, config_dir: &Path) -> Result<()> {
        let p = meta_path(config_dir);
        if let Some(parent) = p.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let tmp = p.with_extension("json.tmp");
        std::fs::write(&tmp, serde_json::to_string_pretty(self)?)?;
        std::fs::rename(&tmp, &p)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o600));
        }
        Ok(())
    }
}

pub async fn wallet_init(config_dir: &Path) -> Result<WalletMeta> {
    if meta_path(config_dir).exists() {
        bail!(
            "wallet already exists at {}",
            meta_path(config_dir).display()
        );
    }
    let dek = require_unlocked(config_dir, "create GRID wallet").await?;
    let _sk = load_operator_signing_key(config_dir, &dek)?;
    let pubkey_hex = operator_pubkey_hex(config_dir)?;
    let address = encode_payment_hex(&pubkey_hex)?;
    let meta = WalletMeta {
        version: 1,
        address: address.clone(),
        pubkey_hex,
        created_at: chrono::Utc::now().to_rfc3339(),
    };
    meta.save(config_dir)?;
    // ensure chain file exists
    let mut chain = ChainState::load(config_dir)?;
    let _ = chain.apply_protocol_burns();
    chain.save(config_dir)?;
    Ok(meta)
}

/// Run chain protocol burns (blockchain only).
pub fn chain_burn_tick(config_dir: &Path) -> Result<(f64, usize)> {
    let mut chain = ChainState::load(config_dir)?;
    let r = chain.apply_protocol_burns();
    chain.save(config_dir)?;
    Ok(r)
}

pub async fn claim_to_wallet(config_dir: &Path, amount_limit: Option<f64>) -> Result<f64> {
    let dek = require_unlocked(config_dir, "claim on-chain").await?;
    let meta = WalletMeta::load(config_dir)?;
    let mut chain = ChainState::load(config_dir)?;
    // protocol burns first (chain)
    let _ = chain.apply_protocol_burns();

    let msg = format!(
        "GRID-CLAIM-v1\n{}\n{}",
        meta.address,
        chrono::Utc::now().to_rfc3339()
    );
    let sig = sign_operator(config_dir, &dek, msg.as_bytes())?;
    let claimed = chain.claim_to_address(&meta.address, amount_limit, None, Some(sig))?;
    chain.save(config_dir)?;
    Ok(claimed)
}

pub async fn send(
    config_dir: &Path,
    to_raw: &str,
    amount: f64,
    memo: Option<String>,
) -> Result<ChainTx> {
    let dek = require_unlocked(config_dir, "send on-chain").await?;
    let meta = WalletMeta::load(config_dir)?;
    let to = normalize_address(to_raw)?;
    let mut chain = ChainState::load(config_dir)?;
    let _ = chain.apply_protocol_burns();

    let at = chrono::Utc::now().to_rfc3339();
    // provisional id in message filled after transfer — sign content without id first
    let msg = format!(
        "GRID-SEND-v1\n{}\n{}\n{:.12}\n{}\n{}",
        meta.address,
        to,
        amount,
        at,
        memo.clone().unwrap_or_default()
    );
    let sig = sign_operator(config_dir, &dek, msg.as_bytes())?;
    let mut tx = chain.transfer(&meta.address, &to, amount, memo, Some(sig))?;
    // re-bind time for consistency
    tx.at = at;
    // replace last tx with signed time if needed — already pushed; ok for phase-1
    chain.save(config_dir)?;
    Ok(tx)
}

pub fn receive_tx(config_dir: &Path, tx: ChainTx, from_pubkey_hex: Option<&str>) -> Result<()> {
    if tx.kind != "send" && tx.kind != "receive" {
        bail!("expected send/receive tx");
    }
    let meta = WalletMeta::load(config_dir)?;
    let to = tx.to.as_deref().context("missing to")?.to_string();
    let to = normalize_address(&to)?;
    if to != meta.address {
        bail!("tx not for this wallet");
    }
    if let (Some(pk), Some(sig_hex), Some(from)) =
        (from_pubkey_hex, tx.signature.as_deref(), tx.from.as_deref())
    {
        let msg = format!(
            "GRID-SEND-v1\n{}\n{}\n{:.12}\n{}\n{}",
            from,
            to,
            tx.amount,
            tx.at,
            tx.memo.clone().unwrap_or_default()
        );
        let pk_bytes = hex::decode(pk)?;
        let vk = VerifyingKey::from_bytes(
            pk_bytes
                .as_slice()
                .try_into()
                .map_err(|_| anyhow::anyhow!("pubkey 32 bytes"))?,
        )?;
        let sig_b = hex::decode(sig_hex)?;
        let sig = Signature::from_bytes(
            sig_b
                .as_slice()
                .try_into()
                .map_err(|_| anyhow::anyhow!("sig 64 bytes"))?,
        );
        vk.verify(msg.as_bytes(), &sig)
            .map_err(|_| anyhow::anyhow!("invalid signature"))?;
    }

    let mut chain = ChainState::load(config_dir)?;
    chain.credit_receive(&to, tx.amount, &tx.id, tx.from, tx.memo, tx.signature)?;
    chain.save(config_dir)?;
    Ok(())
}

pub fn print_burn_banner() {
    print_chain_banner();
}

pub fn print_status(config_dir: &Path) -> Result<()> {
    // Protocol burns run on the chain when status is viewed (advances chain time rules).
    let mut chain = ChainState::load(config_dir)?;
    let (burned, lots) = chain.apply_protocol_burns();
    chain.save(config_dir)?;

    print_chain_banner();
    print_supply(&chain);
    {
        use crate::chain::{format_bytes, ChainFileStats};
        let st = ChainFileStats::collect(config_dir);
        if st.exists {
            println!(
                "  blockchain size: {} ({} bytes) · txs={} · accounts={}",
                format_bytes(st.bytes),
                st.bytes,
                st.txs,
                st.accounts
            );
        } else {
            println!("  blockchain size: 0 B (chain.json not created yet)");
        }
    }
    if burned > 0.0 {
        println!("  protocol burn applied: {burned:.6} GRID ({lots} lot(s))");
    }
    println!();
    println!("  ★ Prefer exit to BTC / other crypto / fiat — do not park forever.");
    println!();

    match WalletMeta::load(config_dir) {
        Ok(m) => {
            println!("Wallet (key material only — balances live on-chain)");
            println!("  address:  {}", m.address);
            println!(
                "  balance:  {:.6} GRID (on-chain)",
                chain.balance(&m.address)
            );
        }
        Err(_) => {
            println!("Wallet not initialized — grid wallet init");
            if let Ok(pk) = operator_pubkey_hex(config_dir) {
                if let Ok(a) = encode_payment_hex(&pk) {
                    println!("  preview:  {a}");
                }
            }
        }
    }

    let path = NodeConfig::path_in(config_dir);
    if path.exists() {
        if let Ok(c) = NodeConfig::load(&path) {
            println!("  node tag: {}", c.node_id);
        }
    }

    let pending = chain.summarize_unclaimed();
    if pending.is_empty() {
        println!();
        println!("  No unclaimed on-chain mint.");
    } else {
        println!();
        println!("Unclaimed mint ON-CHAIN (protocol burns after {BURN_DEADLINE_DAYS}d):");
        for p in &pending {
            let warn = if p.expired_amount > 0.0 {
                " ⚠ PAST DEADLINE — burn on next chain tick"
            } else if p.days_left_min <= 30 {
                " ⚠ claim soon"
            } else {
                ""
            };
            println!(
                "  · node={}  amount={:.6}  jobs={}  days_left≈{}{warn}",
                p.node_id,
                p.amount,
                p.job_count,
                p.days_left_min.max(0)
            );
        }
        println!("  → grid wallet claim   # move unclaimed → your grid0 (on-chain)");
    }

    println!();
    println!("Commands: init | address | claim | send | receive | history | burn-check");
    println!("Burn is chain protocol only — there is no wallet/node burn action.");
    println!("Hard cap: {MAX_SUPPLY:.0} GRID");
    Ok(())
}

pub fn print_history(config_dir: &Path, limit: usize) -> Result<()> {
    let chain = ChainState::load(config_dir)?;
    let n = chain.txs.len().saturating_sub(limit);
    for tx in chain.txs.iter().skip(n).rev() {
        println!(
            "{}  {:8}  {:>12.6}  {} → {}  {}",
            &tx.at[..19.min(tx.at.len())],
            tx.kind,
            tx.amount,
            tx.from.as_deref().unwrap_or("-"),
            tx.to.as_deref().unwrap_or("-"),
            tx.memo.as_deref().unwrap_or(""),
        );
    }
    Ok(())
}

/// CLI helper: dry-run or apply protocol burns on chain.
pub fn burn_check(config_dir: &Path, dry_run: bool) -> Result<(f64, usize)> {
    let mut chain = ChainState::load(config_dir)?;
    let pending = chain.summarize_unclaimed();
    let expired: f64 = pending.iter().map(|p| p.expired_amount).sum();
    println!(
        "On-chain unclaimed: {:.6} GRID",
        pending.iter().map(|p| p.amount).sum::<f64>()
    );
    println!("Past protocol deadline: {expired:.6} GRID");
    print_supply(&chain);
    if dry_run {
        println!("(dry-run — chain not written)");
        return Ok((expired, 0));
    }
    let r = chain.apply_protocol_burns();
    chain.save(config_dir)?;
    println!("✓ chain protocol burn: {:.6} GRID ({} lot(s))", r.0, r.1);
    print_supply(&chain);
    Ok(r)
}

// re-exports used by main
pub use crate::chain::ChainTx as WalletTx;

pub fn pending_mint(
    config_dir: &Path,
    _wallet: Option<&WalletMeta>,
) -> Result<Vec<crate::chain::UnclaimedSummary>> {
    let chain = ChainState::load(config_dir)?;
    Ok(chain.summarize_unclaimed())
}

// compatibility shims removed burn_from_wallet / burn_expired from wallet

pub fn is_valid_grid_address(a: &str) -> bool {
    is_valid_address(a)
}
