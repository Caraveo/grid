//! GRID local blockchain (phase-1 single-operator truth).
//!
//! **Burn is a chain protocol rule — not a wallet action, not a node action.**
//!
//! - Hard supply: **10,000,000,000 GRID**
//! - Work mints → **unclaimed lots** on-chain (bound to a node_id for ops)
//! - `claim` moves lots → `grid0` account balances (on-chain)
//! - **Every** state transition can run `apply_protocol_burns()`:
//!   unclaimed lots older than 365 days are burned **by the chain**,
//!   freeing mint headroom under the 10B cap.
//! - Prefer exiting value to **BTC / other crypto / fiat**.
//!
//! File: `~/.grid/chain.json`

use anyhow::{bail, Context, Result};
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use uuid::Uuid;

use crate::address::{is_valid_address, normalize_address};
use crate::blockchain::ChainReplica;

/// Hard ceiling on circulating GRID.
/// Compute-reward allocation enforced by the pilot reward ledger.
/// Fixed protocol cap: 5B compute-emission allocation + 5B treasury allocation.
pub const MAX_SUPPLY: f64 = 10_000_000_000.0;
/// Lifetime ceiling for verified compute rewards. Treasury GRID is separate.
pub const COMPUTE_ALLOCATION: f64 = 5_000_000_000.0;
/// Unclaimed mint older than this is burned by protocol.
pub const BURN_DEADLINE_DAYS: i64 = 365;
/// Maximum newly-issued GRID per one-hour protocol epoch until governance
/// deliberately changes the signed chain configuration.
pub const DEFAULT_EPOCH_BUDGET: f64 = 25_000.0;
const EMISSION_EPOCH_SECS: i64 = 3600;

const CHAIN_FILE: &str = "chain.json";
const MAX_TX: usize = 50_000;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChainState {
    pub version: u32,
    pub max_supply: f64,
    pub total_minted: f64,
    pub total_burned: f64,
    #[serde(default = "default_epoch_budget")]
    pub epoch_budget: f64,
    #[serde(default)]
    pub emission_epoch: i64,
    #[serde(default)]
    pub epoch_minted: f64,
    /// On-chain unclaimed mint lots (not “node storage” — chain records).
    #[serde(default)]
    pub unclaimed: Vec<UnclaimedLot>,
    /// On-chain account balances by grid0 address.
    #[serde(default)]
    pub accounts: HashMap<String, f64>,
    /// Last accepted Arc transaction nonce by sender address.
    #[serde(default)]
    pub account_nonces: HashMap<String, u64>,
    #[serde(default)]
    pub txs: Vec<ChainTx>,
    /// job_id → claimed|burned
    #[serde(default)]
    pub settled_jobs: HashMap<String, String>,
    pub updated_at: String,
}

fn default_epoch_budget() -> f64 {
    DEFAULT_EPOCH_BUDGET
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UnclaimedLot {
    pub job_id: String,
    /// Operational tag only — burn authority is the chain, not the node.
    pub node_id: String,
    pub amount: f64,
    pub commitment: String,
    pub minted_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChainTx {
    pub id: String,
    /// mint | claim | send | burn
    pub kind: String,
    pub at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub from: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub to: Option<String>,
    pub amount: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub memo: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signature: Option<String>,
}

#[derive(Debug, Clone)]
pub struct UnclaimedSummary {
    pub node_id: String,
    pub amount: f64,
    pub job_count: usize,
    pub oldest: DateTime<Utc>,
    pub expired_amount: f64,
    pub days_left_min: i64,
}

fn now_rfc() -> String {
    Utc::now().to_rfc3339()
}

fn parse_ts(s: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(s)
        .ok()
        .map(|d| d.with_timezone(&Utc))
}

impl Default for ChainState {
    fn default() -> Self {
        Self {
            version: 1,
            max_supply: MAX_SUPPLY,
            total_minted: 0.0,
            total_burned: 0.0,
            epoch_budget: DEFAULT_EPOCH_BUDGET,
            emission_epoch: Utc::now().timestamp().div_euclid(EMISSION_EPOCH_SECS),
            epoch_minted: 0.0,
            unclaimed: vec![],
            accounts: HashMap::new(),
            account_nonces: HashMap::new(),
            txs: vec![],
            settled_jobs: HashMap::new(),
            updated_at: now_rfc(),
        }
    }
}

impl ChainState {
    pub fn path_in(config_dir: &Path) -> PathBuf {
        config_dir.join(CHAIN_FILE)
    }

    pub fn load(config_dir: &Path) -> Result<Self> {
        let p = Self::path_in(config_dir);
        if !p.exists() {
            return Ok(Self::default());
        }
        let raw = std::fs::read_to_string(&p)?;
        let mut s: Self = serde_json::from_str(&raw)?;
        // Protocol constants win over stale pilot files after an allocation update.
        s.max_supply = MAX_SUPPLY;
        s.epoch_budget = DEFAULT_EPOCH_BUDGET;
        Ok(s)
    }

    pub fn save(&self, config_dir: &Path) -> Result<()> {
        let p = Self::path_in(config_dir);
        if let Some(parent) = p.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut s = self.clone();
        s.updated_at = now_rfc();
        let tmp = p.with_extension("json.tmp");
        std::fs::write(&tmp, serde_json::to_string_pretty(&s)?)
            .with_context(|| format!("write {}", tmp.display()))?;
        std::fs::rename(&tmp, &p)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o600));
        }
        Ok(())
    }

    pub fn circulating(&self) -> f64 {
        (self.total_minted - self.total_burned).max(0.0)
    }

    pub fn mint_headroom(&self) -> f64 {
        let total_headroom = (self.max_supply - self.circulating()).max(0.0);
        let compute_issued = (self.total_minted - self.total_burned).max(0.0);
        total_headroom.min((COMPUTE_ALLOCATION - compute_issued).max(0.0))
    }

    pub fn next_account_nonce(&self, address: &str) -> Result<u64> {
        let address = normalize_address(address)?;
        Ok(self
            .account_nonces
            .get(&address)
            .copied()
            .unwrap_or(0)
            .saturating_add(1))
    }

    pub fn commit_account_nonce(&mut self, address: &str, nonce: u64) -> Result<()> {
        let address = normalize_address(address)?;
        let expected = self.next_account_nonce(&address)?;
        if nonce != expected {
            bail!("invalid nonce: expected {expected}");
        }
        self.account_nonces.insert(address, nonce);
        Ok(())
    }

    fn refresh_emission_epoch(&mut self) {
        let current = Utc::now().timestamp().div_euclid(EMISSION_EPOCH_SECS);
        if self.emission_epoch != current {
            self.emission_epoch = current;
            self.epoch_minted = 0.0;
        }
    }

    pub fn epoch_headroom(&mut self) -> f64 {
        self.refresh_emission_epoch();
        (self.epoch_budget - self.epoch_minted).max(0.0)
    }

    pub fn balance(&self, addr: &str) -> f64 {
        self.accounts.get(addr).copied().unwrap_or(0.0)
    }

    fn push_tx(&mut self, tx: ChainTx) {
        self.txs.push(tx);
        if self.txs.len() > MAX_TX {
            let n = self.txs.len() - MAX_TX;
            self.txs.drain(0..n);
        }
    }

    /// **Protocol burn** — only place credits are destroyed.
    /// Burns unclaimed lots older than [`BURN_DEADLINE_DAYS`].
    /// Returns (amount_burned, lots_burned).
    pub fn apply_protocol_burns(&mut self) -> (f64, usize) {
        let now = Utc::now();
        let deadline = Duration::days(BURN_DEADLINE_DAYS);
        let mut burned = 0.0f64;
        let mut lots = 0usize;
        let mut keep = Vec::new();

        for lot in std::mem::take(&mut self.unclaimed) {
            if self.settled_jobs.contains_key(&lot.job_id) {
                continue;
            }
            let at = parse_ts(&lot.minted_at).unwrap_or(now);
            if now.signed_duration_since(at) >= deadline {
                burned += lot.amount;
                lots += 1;
                self.total_burned += lot.amount;
                self.settled_jobs
                    .insert(lot.job_id.clone(), "burned".into());
                self.push_tx(ChainTx {
                    id: format!("burn_{}", Uuid::new_v4()),
                    kind: "burn".into(),
                    at: now_rfc(),
                    from: Some(format!("unclaimed:{}", lot.node_id)),
                    to: None,
                    amount: lot.amount,
                    memo: Some(format!(
                        "PROTOCOL BURN unclaimed >{BURN_DEADLINE_DAYS}d job={} (chain rule, not wallet/node)",
                        lot.job_id
                    )),
                    signature: None,
                });
            } else {
                keep.push(lot);
            }
        }
        self.unclaimed = keep;
        (burned, lots)
    }

    /// Mint work reward as an unclaimed pilot lot (capped by the 5B compute allocation).
    /// Returns actual amount minted (0 if cap full).
    pub fn mint_unclaimed(
        &mut self,
        node_id: &str,
        job_id: &str,
        amount: f64,
        commitment: &str,
    ) -> f64 {
        // Always run protocol burns first so headroom can reopen.
        let _ = self.apply_protocol_burns();
        let epoch_headroom = self.epoch_headroom();

        if amount <= 0.0 || !amount.is_finite() {
            return 0.0;
        }
        if self.settled_jobs.contains_key(job_id) {
            return 0.0;
        }
        if self.unclaimed.iter().any(|l| l.job_id == job_id) {
            return 0.0;
        }
        let actual = amount.min(self.mint_headroom()).min(epoch_headroom);
        if actual <= 0.0 {
            return 0.0;
        }
        self.total_minted += actual;
        self.epoch_minted += actual;
        let at = now_rfc();
        self.unclaimed.push(UnclaimedLot {
            job_id: job_id.into(),
            node_id: node_id.into(),
            amount: actual,
            commitment: commitment.into(),
            minted_at: at.clone(),
        });
        self.push_tx(ChainTx {
            id: format!("mint_{}", Uuid::new_v4()),
            kind: "mint".into(),
            at,
            from: None,
            to: Some(format!("unclaimed:{node_id}")),
            amount: actual,
            memo: Some(format!("job {job_id}")),
            signature: None,
        });
        actual
    }

    /// Move unclaimed lots → grid0 account (on-chain claim).
    pub fn claim_to_address(
        &mut self,
        address: &str,
        amount_limit: Option<f64>,
        node_filter: Option<&str>,
        signature: Option<String>,
    ) -> Result<f64> {
        let addr = normalize_address(address)?;
        let _ = self.apply_protocol_burns();

        let now = Utc::now();
        let deadline = Duration::days(BURN_DEADLINE_DAYS);
        let limit = amount_limit.unwrap_or(f64::MAX);
        let mut claimed = 0.0f64;
        let mut keep = Vec::new();

        for lot in std::mem::take(&mut self.unclaimed) {
            if self.settled_jobs.contains_key(&lot.job_id) {
                continue;
            }
            if let Some(nf) = node_filter {
                if lot.node_id != nf {
                    keep.push(lot);
                    continue;
                }
            }
            let at = parse_ts(&lot.minted_at).unwrap_or(now);
            // expired lots stay for protocol burn (or burn now)
            if now.signed_duration_since(at) >= deadline {
                keep.push(lot);
                continue;
            }
            if claimed + lot.amount > limit + 1e-12 {
                keep.push(lot);
                continue;
            }
            *self.accounts.entry(addr.clone()).or_insert(0.0) += lot.amount;
            claimed += lot.amount;
            self.settled_jobs
                .insert(lot.job_id.clone(), "claimed".into());
            self.push_tx(ChainTx {
                id: format!("claim_{}", Uuid::new_v4()),
                kind: "claim".into(),
                at: now_rfc(),
                from: Some(format!("unclaimed:{}", lot.node_id)),
                to: Some(addr.clone()),
                amount: lot.amount,
                memo: Some(format!("job {}", lot.job_id)),
                signature: signature.clone(),
            });
        }
        self.unclaimed = keep;
        // burn any that expired while we held the lock
        let _ = self.apply_protocol_burns();
        Ok(claimed)
    }

    /// Transfer between grid0 accounts on-chain.
    pub fn transfer(
        &mut self,
        from: &str,
        to: &str,
        amount: f64,
        memo: Option<String>,
        signature: Option<String>,
    ) -> Result<ChainTx> {
        if amount <= 0.0 || !amount.is_finite() {
            bail!("amount must be positive");
        }
        let from = normalize_address(from)?;
        let to = normalize_address(to)?;
        if from == to {
            bail!("cannot send to self");
        }
        if !is_valid_address(&to) {
            bail!("invalid destination address");
        }
        let _ = self.apply_protocol_burns();

        let bal = self.balance(&from);
        if amount > bal + 1e-12 {
            bail!("insufficient on-chain balance: have {bal:.6}, need {amount:.6}");
        }
        *self.accounts.entry(from.clone()).or_insert(0.0) -= amount;
        *self.accounts.entry(to.clone()).or_insert(0.0) += amount;

        let tx = ChainTx {
            id: format!("send_{}", Uuid::new_v4()),
            kind: "send".into(),
            at: now_rfc(),
            from: Some(from),
            to: Some(to),
            amount,
            memo,
            signature,
        };
        self.push_tx(tx.clone());
        Ok(tx)
    }

    /// Credit a receive (import) onto an account — amount already left sender chain.
    pub fn credit_receive(
        &mut self,
        to: &str,
        amount: f64,
        tx_id: &str,
        from: Option<String>,
        memo: Option<String>,
        signature: Option<String>,
    ) -> Result<()> {
        let to = normalize_address(to)?;
        if self.txs.iter().any(|t| t.id == tx_id) {
            return Ok(());
        }
        if amount <= 0.0 {
            bail!("invalid amount");
        }
        let _ = self.apply_protocol_burns();
        // Receive does not mint — pure transfer import. Cap check N/A.
        *self.accounts.entry(to.clone()).or_insert(0.0) += amount;
        self.push_tx(ChainTx {
            id: tx_id.into(),
            kind: "receive".into(),
            at: now_rfc(),
            from,
            to: Some(to),
            amount,
            memo,
            signature,
        });
        Ok(())
    }

    pub fn summarize_unclaimed(&self) -> Vec<UnclaimedSummary> {
        let now = Utc::now();
        let deadline = Duration::days(BURN_DEADLINE_DAYS);
        let mut by: HashMap<String, Vec<&UnclaimedLot>> = HashMap::new();
        for lot in &self.unclaimed {
            if self.settled_jobs.contains_key(&lot.job_id) {
                continue;
            }
            by.entry(lot.node_id.clone()).or_default().push(lot);
        }
        let mut out = Vec::new();
        for (node_id, lots) in by {
            let mut amount = 0.0;
            let mut expired = 0.0;
            let mut oldest = now;
            let mut min_left = BURN_DEADLINE_DAYS;
            for l in &lots {
                amount += l.amount;
                let at = parse_ts(&l.minted_at).unwrap_or(now);
                if at < oldest {
                    oldest = at;
                }
                let age = now.signed_duration_since(at);
                let left = (deadline - age).num_days();
                if left < min_left {
                    min_left = left;
                }
                if age >= deadline {
                    expired += l.amount;
                }
            }
            out.push(UnclaimedSummary {
                node_id,
                amount,
                job_count: lots.len(),
                oldest,
                expired_amount: expired,
                days_left_min: min_left,
            });
        }
        out.sort_by(|a, b| a.node_id.cmp(&b.node_id));
        out
    }
}

pub fn print_chain_banner() {
    println!();
    println!("╔══════════════════════════════════════════════════════════════════╗");
    println!("║  GRID CHAIN · hard supply 10,000,000,000                        ║");
    println!("║  BURN is a PROTOCOL rule on the blockchain — not wallet, not    ║");
    println!("║  node. Unclaimed mint lots older than {BURN_DEADLINE_DAYS} days are burned.     ║");
    println!("║  Burns free mint headroom so the 10B cap is never “full” of     ║");
    println!("║  dead coins.                                                    ║");
    println!("║  BEST: exit value to BTC / other crypto / fiat.                 ║");
    println!("╚══════════════════════════════════════════════════════════════════╝");
    println!();
}

pub fn print_supply(chain: &ChainState) {
    println!("  chain file:    chain.json (blockchain truth)");
    println!("  supply cap:    {:.0} GRID", chain.max_supply);
    println!("  circulating:   {:.6}", chain.circulating());
    println!("  minted life:   {:.6}", chain.total_minted);
    println!(
        "  burned life:   {:.6}  (protocol burns)",
        chain.total_burned
    );
    println!("  mint headroom: {:.6}", chain.mint_headroom());
    println!("  unclaimed lots:{}", chain.unclaimed.len());
    println!("  accounts:      {}", chain.accounts.len());
}

// ── size + security ──────────────────────────────────────────────────

/// Human-readable byte size (IEC-style, binary).
pub fn format_bytes(n: u64) -> String {
    const UNITS: [&str; 6] = ["B", "KiB", "MiB", "GiB", "TiB", "PiB"];
    if n < 1024 {
        return format!("{n} B");
    }
    let mut v = n as f64;
    let mut i = 0usize;
    while v >= 1024.0 && i < UNITS.len() - 1 {
        v /= 1024.0;
        i += 1;
    }
    if i == 0 {
        format!("{n} B")
    } else {
        format!("{v:.2} {}", UNITS[i])
    }
}

#[derive(Debug, Clone)]
pub struct ChainFileStats {
    pub path: PathBuf,
    pub exists: bool,
    pub bytes: u64,
    /// Unix mode bits if available (e.g. 0o600).
    pub mode: Option<u32>,
    pub version: u32,
    pub txs: usize,
    pub accounts: usize,
    pub unclaimed_lots: usize,
    pub settled_jobs: usize,
    pub max_supply: f64,
    pub circulating: f64,
    pub total_minted: f64,
    pub total_burned: f64,
    pub mint_headroom: f64,
    pub updated_at: String,
    /// Sum of all account balances.
    pub balances_sum: f64,
    /// Sum of unclaimed lot amounts.
    pub unclaimed_sum: f64,
}

impl ChainFileStats {
    pub fn collect(config_dir: &Path) -> Self {
        let path = ChainState::path_in(config_dir);
        let (exists, bytes, mode) = if path.exists() {
            let meta = std::fs::metadata(&path).ok();
            let bytes = meta.as_ref().map(|m| m.len()).unwrap_or(0);
            #[cfg(unix)]
            let mode = meta.map(|m| {
                use std::os::unix::fs::PermissionsExt;
                m.permissions().mode() & 0o777
            });
            #[cfg(not(unix))]
            let mode = None;
            (true, bytes, mode)
        } else {
            (false, 0u64, None)
        };

        let chain = ChainState::load(config_dir).unwrap_or_default();
        let balances_sum: f64 = chain.accounts.values().sum();
        let unclaimed_sum: f64 = chain.unclaimed.iter().map(|l| l.amount).sum();

        Self {
            path,
            exists,
            bytes,
            mode,
            version: chain.version,
            txs: chain.txs.len(),
            accounts: chain.accounts.len(),
            unclaimed_lots: chain.unclaimed.len(),
            settled_jobs: chain.settled_jobs.len(),
            max_supply: chain.max_supply,
            circulating: chain.circulating(),
            total_minted: chain.total_minted,
            total_burned: chain.total_burned,
            mint_headroom: chain.mint_headroom(),
            updated_at: chain.updated_at.clone(),
            balances_sum,
            unclaimed_sum,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SecLevel {
    Ok,
    Warn,
    Fail,
}

#[derive(Debug, Clone)]
pub struct SecFinding {
    pub level: SecLevel,
    pub code: &'static str,
    pub message: String,
}

/// Local chain + key material security audit (operator machine).
pub fn security_audit(config_dir: &Path) -> Vec<SecFinding> {
    let mut out = Vec::new();
    let stats = ChainFileStats::collect(config_dir);
    let chain = ChainState::load(config_dir).unwrap_or_default();

    // --- chain file ---
    if !stats.exists {
        out.push(SecFinding {
            level: SecLevel::Warn,
            code: "chain.missing",
            message: format!(
                "chain.json not created yet (will appear at {})",
                stats.path.display()
            ),
        });
    } else {
        out.push(SecFinding {
            level: SecLevel::Ok,
            code: "chain.present",
            message: format!(
                "blockchain file {} ({})",
                stats.path.display(),
                format_bytes(stats.bytes)
            ),
        });
        #[cfg(unix)]
        if let Some(mode) = stats.mode {
            if mode & 0o077 != 0 {
                out.push(SecFinding {
                    level: SecLevel::Fail,
                    code: "chain.perms",
                    message: format!(
                        "chain.json mode {:o} is world/group readable — expected 600",
                        mode
                    ),
                });
            } else if mode != 0o600 {
                out.push(SecFinding {
                    level: SecLevel::Warn,
                    code: "chain.perms",
                    message: format!("chain.json mode {:o} (prefer 600)", mode),
                });
            } else {
                out.push(SecFinding {
                    level: SecLevel::Ok,
                    code: "chain.perms",
                    message: "chain.json permissions 600".into(),
                });
            }
        }
    }

    // --- supply integrity ---
    if (chain.max_supply - MAX_SUPPLY).abs() > 1e-6 {
        out.push(SecFinding {
            level: SecLevel::Fail,
            code: "supply.cap",
            message: format!(
                "max_supply is {:.0} — protocol hard cap is {:.0}",
                chain.max_supply, MAX_SUPPLY
            ),
        });
    } else {
        out.push(SecFinding {
            level: SecLevel::Ok,
            code: "supply.cap",
            message: format!("hard supply cap {:.0} GRID", MAX_SUPPLY),
        });
    }

    if chain.total_minted < 0.0
        || chain.total_burned < 0.0
        || !chain.total_minted.is_finite()
        || !chain.total_burned.is_finite()
    {
        out.push(SecFinding {
            level: SecLevel::Fail,
            code: "supply.nan",
            message: "minted/burned totals are invalid (negative or non-finite)".into(),
        });
    }

    if chain.circulating() > chain.max_supply + 1e-6 {
        out.push(SecFinding {
            level: SecLevel::Fail,
            code: "supply.overcap",
            message: format!(
                "circulating {:.6} exceeds cap {:.0}",
                chain.circulating(),
                chain.max_supply
            ),
        });
    }

    // accounts + unclaimed should match circulating (minted − burned)
    let held = stats.balances_sum + stats.unclaimed_sum;
    let circ = chain.circulating();
    let drift = (held - circ).abs();
    if drift > 1e-3 {
        out.push(SecFinding {
            level: SecLevel::Fail,
            code: "supply.drift",
            message: format!(
                "balance+unclaimed ({held:.6}) ≠ circulating ({circ:.6}) drift={drift:.6}"
            ),
        });
    } else {
        out.push(SecFinding {
            level: SecLevel::Ok,
            code: "supply.ledger",
            message: format!(
                "ledger closed: accounts {:.6} + unclaimed {:.6} = circulating {:.6}",
                stats.balances_sum, stats.unclaimed_sum, circ
            ),
        });
    }

    for (addr, bal) in &chain.accounts {
        if *bal < -1e-9 || !bal.is_finite() {
            out.push(SecFinding {
                level: SecLevel::Fail,
                code: "account.negative",
                message: format!("invalid balance on {addr}: {bal}"),
            });
        }
        if !is_valid_address(addr) {
            out.push(SecFinding {
                level: SecLevel::Warn,
                code: "account.address",
                message: format!("non-canonical address in ledger: {addr}"),
            });
        }
    }

    // negative / oversized unclaimed lots
    for lot in &chain.unclaimed {
        if lot.amount <= 0.0 || !lot.amount.is_finite() {
            out.push(SecFinding {
                level: SecLevel::Fail,
                code: "unclaimed.bad",
                message: format!("invalid unclaimed lot {}: {}", lot.job_id, lot.amount),
            });
        }
    }

    // tx log bound
    if chain.txs.len() >= MAX_TX {
        out.push(SecFinding {
            level: SecLevel::Warn,
            code: "txs.cap",
            message: format!("tx log at capacity ({MAX_TX}) — oldest txs are pruned on write"),
        });
    } else {
        out.push(SecFinding {
            level: SecLevel::Ok,
            code: "txs.log",
            message: format!("tx log {} / {MAX_TX} entries", chain.txs.len()),
        });
    }

    // Canonical network truth is the signed Genesis replica. Local chain.json
    // remains operational state, so report the model based on actual evidence
    // instead of emitting an unconditional warning on every healthy peer.
    match ChainReplica::load(config_dir) {
        Ok(Some(replica)) => match replica.verify() {
            Ok(()) => out.push(SecFinding {
                level: SecLevel::Ok,
                code: "model.genesis",
                message: format!(
                    "following Genesis-signed canonical chain {} at height {}; local chain data is operational state",
                    replica.chain_id,
                    replica.tip().height
                ),
            }),
            Err(error) => out.push(SecFinding {
                level: SecLevel::Fail,
                code: "model.replica",
                message: format!("Genesis-signed chain replica failed verification: {error}"),
            }),
        },
        Ok(None) => out.push(SecFinding {
            level: SecLevel::Warn,
            code: "model.replica",
            message: "no Genesis-signed chain replica is stored yet; start the node and verify Genesis connectivity"
                .into(),
        }),
        Err(error) => out.push(SecFinding {
            level: SecLevel::Fail,
            code: "model.replica",
            message: format!("could not load Genesis-signed chain replica: {error}"),
        }),
    }

    // --- keys / secrets under config dir ---
    audit_path_perms(&mut out, config_dir.join("keys"), "keys.dir", true);
    for name in [
        "keys/admin.secret",
        "keys/ca.seed",
        "keys/webhook.secret",
        "keys/operator.secret",
        "keys/wallet.secret",
        "wallet.json",
        "operator.key",
    ] {
        let p = config_dir.join(name);
        if p.exists() {
            audit_path_perms(&mut out, p, "keys.file", false);
        }
    }

    // config dir itself
    if config_dir.exists() {
        #[cfg(unix)]
        {
            if let Ok(meta) = std::fs::metadata(config_dir) {
                use std::os::unix::fs::PermissionsExt;
                let mode = meta.permissions().mode() & 0o777;
                if mode & 0o077 != 0 {
                    out.push(SecFinding {
                        level: SecLevel::Warn,
                        code: "config.perms",
                        message: format!(
                            "{} mode {:o} — prefer 700 for operator home",
                            config_dir.display(),
                            mode
                        ),
                    });
                } else {
                    out.push(SecFinding {
                        level: SecLevel::Ok,
                        code: "config.perms",
                        message: format!("config dir mode {:o}", mode),
                    });
                }
            }
        }
    }

    // registry URL should be https
    let reg =
        std::env::var("GRID_REGISTRY_URL").unwrap_or_else(|_| "https://grid-compute.com".into());
    if reg.starts_with("https://") {
        out.push(SecFinding {
            level: SecLevel::Ok,
            code: "registry.tls",
            message: format!("registry URL uses TLS ({reg})"),
        });
    } else {
        out.push(SecFinding {
            level: SecLevel::Fail,
            code: "registry.tls",
            message: format!("registry URL is not HTTPS: {reg}"),
        });
    }

    out
}

#[cfg(unix)]
fn audit_path_perms(out: &mut Vec<SecFinding>, path: PathBuf, code: &'static str, is_dir: bool) {
    use std::os::unix::fs::PermissionsExt;
    let Ok(meta) = std::fs::metadata(&path) else {
        return;
    };
    let mode = meta.permissions().mode() & 0o777;
    let want = if is_dir { 0o700 } else { 0o600 };
    if mode & 0o077 != 0 {
        out.push(SecFinding {
            level: SecLevel::Fail,
            code,
            message: format!(
                "{} mode {:o} is group/world accessible — expected {:o}",
                path.display(),
                mode,
                want
            ),
        });
    } else if mode != want {
        out.push(SecFinding {
            level: SecLevel::Warn,
            code,
            message: format!("{} mode {:o} (prefer {:o})", path.display(), mode, want),
        });
    } else {
        out.push(SecFinding {
            level: SecLevel::Ok,
            code,
            message: format!("{} mode {:o}", path.display(), mode),
        });
    }
}

#[cfg(not(unix))]
fn audit_path_perms(
    _out: &mut Vec<SecFinding>,
    _path: PathBuf,
    _code: &'static str,
    _is_dir: bool,
) {
}

/// Print blockchain size + security section for `grid status`.
pub fn print_status_blockchain(config_dir: &Path) {
    let stats = ChainFileStats::collect(config_dir);
    let findings = security_audit(config_dir);

    println!();
    println!("=== Blockchain ===");
    if stats.exists {
        println!(
            "  file:         {}  ({})",
            stats.path.display(),
            format_bytes(stats.bytes)
        );
        #[cfg(unix)]
        if let Some(mode) = stats.mode {
            println!("  perms:        {:o}", mode);
        }
        println!("  size:         {} bytes", stats.bytes);
    } else {
        println!(
            "  file:         {}  (not created yet)",
            stats.path.display()
        );
        println!("  size:         0 B");
    }
    println!("  version:      {}", stats.version);
    println!("  supply cap:   {:.0} GRID", stats.max_supply);
    println!("  circulating:  {:.6} GRID", stats.circulating);
    println!("  minted:       {:.6}", stats.total_minted);
    println!("  burned:       {:.6}  (protocol)", stats.total_burned);
    println!("  headroom:     {:.6}", stats.mint_headroom);
    println!(
        "  accounts:     {}  (Σ {:.6} GRID)",
        stats.accounts, stats.balances_sum
    );
    println!(
        "  unclaimed:    {} lots  (Σ {:.6} GRID)",
        stats.unclaimed_lots, stats.unclaimed_sum
    );
    println!("  txs:          {}", stats.txs);
    println!("  settled jobs: {}", stats.settled_jobs);
    if !stats.updated_at.is_empty() {
        println!("  updated:      {}", stats.updated_at);
    }
    println!("  burn rule:    unclaimed mint >{BURN_DEADLINE_DAYS}d burned by chain protocol");
    if let Some(model) = findings
        .iter()
        .find(|finding| finding.code.starts_with("model."))
    {
        println!("  model:        {}", model.message);
    }

    println!();
    println!("=== Security check ===");
    let mut ok = 0usize;
    let mut warn = 0usize;
    let mut fail = 0usize;
    for f in &findings {
        let tag = match f.level {
            SecLevel::Ok => {
                ok += 1;
                "OK  "
            }
            SecLevel::Warn => {
                warn += 1;
                "WARN"
            }
            SecLevel::Fail => {
                fail += 1;
                "FAIL"
            }
        };
        println!("  [{tag}] {}: {}", f.code, f.message);
    }
    println!();
    if fail > 0 {
        println!("  result: FAIL — {fail} failure(s), {warn} warning(s), {ok} ok");
    } else if warn > 0 {
        println!("  result: WARN — {warn} warning(s), {ok} ok (no failures)");
    } else {
        println!("  result: OK — {ok} checks passed");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn mint_claim_and_protocol_burn() {
        let dir = tempdir().unwrap();
        let mut c = ChainState::default();
        let m = c.mint_unclaimed("node_a", "job1", 100.0, "c1");
        assert!((m - 100.0).abs() < 1e-9);
        assert_eq!(c.unclaimed.len(), 1);

        // force lot old
        c.unclaimed[0].minted_at = (Utc::now() - Duration::days(400)).to_rfc3339();
        let (b, n) = c.apply_protocol_burns();
        assert_eq!(n, 1);
        assert!((b - 100.0).abs() < 1e-9);
        assert!((c.mint_headroom() - COMPUTE_ALLOCATION).abs() < 1.0);
        assert!((c.circulating()).abs() < 1e-9);

        c.save(dir.path()).unwrap();
        let c2 = ChainState::load(dir.path()).unwrap();
        assert!((c2.total_burned - 100.0).abs() < 1e-9);
    }

    #[test]
    fn cap_blocks_mint() {
        let mut c = ChainState::default();
        c.total_minted = COMPUTE_ALLOCATION;
        c.total_burned = 0.0;
        let m = c.mint_unclaimed("n", "j", 50.0, "c");
        assert_eq!(m, 0.0);
        c.total_burned = 50.0;
        let m2 = c.mint_unclaimed("n", "j2", 50.0, "c");
        assert!((m2 - 50.0).abs() < 1e-9);
    }

    #[test]
    fn account_nonce_is_monotonic_and_replay_safe() {
        let key = ed25519_dalek::SigningKey::from_bytes(&[3u8; 32]);
        let address =
            crate::address::encode_payment(key.verifying_key().as_bytes()).expect("address");
        let mut chain = ChainState::default();
        assert_eq!(chain.next_account_nonce(&address).unwrap(), 1);
        chain.commit_account_nonce(&address, 1).unwrap();
        assert_eq!(chain.next_account_nonce(&address).unwrap(), 2);
        assert!(chain.commit_account_nonce(&address, 1).is_err());
        assert!(chain.commit_account_nonce(&address, 3).is_err());
    }

    #[test]
    fn format_bytes_units() {
        assert_eq!(format_bytes(0), "0 B");
        assert_eq!(format_bytes(512), "512 B");
        assert_eq!(format_bytes(1024), "1.00 KiB");
        assert_eq!(format_bytes(1536), "1.50 KiB");
    }

    #[test]
    fn security_audit_default_chain_ok() {
        let dir = tempdir().unwrap();
        let findings = security_audit(dir.path());
        assert!(
            findings
                .iter()
                .any(|f| f.code == "supply.cap" && f.level == SecLevel::Ok),
            "{findings:?}"
        );
        assert!(
            findings
                .iter()
                .any(|f| f.code == "supply.ledger" && f.level == SecLevel::Ok),
            "{findings:?}"
        );
        assert!(
            findings
                .iter()
                .any(|f| f.code == "model.replica" && f.level == SecLevel::Warn),
            "{findings:?}"
        );
    }

    #[test]
    fn security_audit_accepts_verified_genesis_replica() {
        let dir = tempdir().unwrap();
        let signing = ed25519_dalek::SigningKey::from_bytes(&[11u8; 32]);
        let keys = crate::genesis::GenesisKeys {
            verifying: signing.verifying_key(),
            signing,
        };
        let replica = crate::blockchain::ChainReplica::create_genesis(&keys, vec![]).unwrap();
        replica.save(dir.path()).unwrap();

        let findings = security_audit(dir.path());
        assert!(
            findings
                .iter()
                .any(|f| f.code == "model.genesis" && f.level == SecLevel::Ok),
            "{findings:?}"
        );
    }
}
