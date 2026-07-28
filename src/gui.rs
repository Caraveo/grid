//! Stable local JSON contract shared by the native wallet applications.
//! Secret-bearing requests arrive over stdin; snapshots never include secrets.

use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};
use std::path::Path;

use crate::chain::{ChainState, ChainTx, BURN_DEADLINE_DAYS};
use crate::config::NodeConfig;
use crate::passkey::{
    auth_init, auth_init_combo_gui, auth_init_keyphrase_gui, auth_init_password_gui, auth_status,
    auth_unlock_gui, AuthMode,
};
use crate::wallet::{claim_to_wallet, send, wallet_init, WalletMeta};

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WalletSnapshot {
    pub version: u32,
    pub config_dir: String,
    pub auth: AuthSnapshot,
    pub grid: GridSnapshot,
    pub solana: SolanaSnapshot,
    pub bitcoin: BitcoinSnapshot,
    pub activity: Vec<ChainTx>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthSnapshot {
    pub initialized: bool,
    pub mode: String,
    pub encrypted: bool,
    pub unlocked: bool,
    pub passkey_registered: bool,
    pub detail: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GridSnapshot {
    pub initialized: bool,
    pub address: Option<String>,
    pub balance: f64,
    pub unclaimed: f64,
    pub total_minted: f64,
    pub total_burned: f64,
    pub max_supply: f64,
    pub burn_deadline_days: i64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SolanaSnapshot {
    pub configured: bool,
    pub address: Option<String>,
    pub balance: Option<f64>,
    pub network: String,
    pub custody: Option<String>,
    pub mint: String,
    pub error: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BitcoinSnapshot {
    pub network: String,
    pub role: String,
    pub route: String,
    pub live: bool,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "action", rename_all = "camelCase")]
pub enum WalletAction {
    SetupKeyphrase,
    SetupPassword {
        password: String,
    },
    SetupPasskey,
    SetupCombo {
        password: String,
    },
    Unlock {
        password: Option<String>,
        keyphrase: Option<String>,
    },
    InitializeGrid,
    Claim {
        amount: Option<f64>,
    },
    Send {
        to: String,
        amount: f64,
        memo: Option<String>,
    },
    CreateSolana,
    ImportSolana {
        address: String,
    },
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActionResult {
    pub ok: bool,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recovery_phrase: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub transaction: Option<ChainTx>,
    pub snapshot: WalletSnapshot,
}

pub async fn snapshot(config_dir: &Path) -> WalletSnapshot {
    let auth = auth_status(config_dir);
    let chain = ChainState::load(config_dir).unwrap_or_default();
    let wallet = WalletMeta::load(config_dir).ok();
    let address = wallet.as_ref().map(|value| value.address.clone());
    let balance = address
        .as_deref()
        .map(|value| chain.balance(value))
        .unwrap_or(0.0);
    let unclaimed = chain
        .unclaimed
        .iter()
        .map(|lot| lot.amount)
        .sum::<f64>()
        .max(0.0);
    let config_path = NodeConfig::path_in(config_dir);
    let configured_address = NodeConfig::load(&config_path)
        .ok()
        .and_then(|config| config.solana_reward_wallet);
    let solana = if configured_address.is_some() {
        match crate::solana_wallet::balance(config_dir).await {
            Ok((address, value, local)) => SolanaSnapshot {
                configured: true,
                address: Some(address),
                balance: Some(value),
                network: "devnet".into(),
                custody: Some(if local { "local" } else { "external" }.into()),
                mint: crate::solana_wallet::GRID_DEVNET_MINT.into(),
                error: None,
            },
            Err(error) => SolanaSnapshot {
                configured: true,
                address: configured_address,
                balance: None,
                network: "devnet".into(),
                custody: None,
                mint: crate::solana_wallet::GRID_DEVNET_MINT.into(),
                error: Some(error.to_string()),
            },
        }
    } else {
        SolanaSnapshot {
            configured: false,
            address: None,
            balance: None,
            network: "devnet".into(),
            custody: None,
            mint: crate::solana_wallet::GRID_DEVNET_MINT.into(),
            error: None,
        }
    };
    let mut activity = chain.txs.clone();
    activity.reverse();
    activity.truncate(100);
    WalletSnapshot {
        version: 1,
        config_dir: config_dir.display().to_string(),
        auth: AuthSnapshot {
            initialized: auth.mode != "none",
            mode: auth.mode,
            encrypted: auth.keys_encrypted,
            unlocked: auth.session_unlocked,
            passkey_registered: auth.passkey_registered,
            detail: auth.detail,
        },
        grid: GridSnapshot {
            initialized: wallet.is_some(),
            address,
            balance,
            unclaimed,
            total_minted: chain.total_minted,
            total_burned: chain.total_burned,
            max_supply: chain.max_supply,
            burn_deadline_days: BURN_DEADLINE_DAYS,
        },
        solana,
        bitcoin: BitcoinSnapshot {
            network: "bitcoin".into(),
            role: "Transact Security Layer".into(),
            route: "GRID → SOL → BTC".into(),
            live: false,
        },
        activity,
    }
}

pub async fn act(config_dir: &Path, action: WalletAction) -> Result<ActionResult> {
    let mut phrase = None;
    let mut transaction = None;
    let message = match action {
        WalletAction::SetupKeyphrase => {
            phrase = Some(auth_init_keyphrase_gui(config_dir)?);
            "Encrypted 24-word GRID vault created".into()
        }
        WalletAction::SetupPassword { password } => {
            auth_init_password_gui(config_dir, &password)?;
            "Encrypted password GRID vault created".into()
        }
        WalletAction::SetupPasskey => {
            auth_init(config_dir, AuthMode::Passkey).await?;
            "Passkey-protected GRID vault created".into()
        }
        WalletAction::SetupCombo { password } => {
            phrase = Some(auth_init_combo_gui(config_dir, &password).await?);
            "Password + passkey + keyphrase GRID vault created".into()
        }
        WalletAction::Unlock {
            password,
            keyphrase,
        } => {
            auth_unlock_gui(config_dir, password.as_deref(), keyphrase.as_deref()).await?;
            "GRID vault unlocked for eight hours".into()
        }
        WalletAction::InitializeGrid => {
            ensure_unlocked(config_dir)?;
            let wallet = wallet_init(config_dir).await?;
            format!("GRID wallet created: {}", wallet.address)
        }
        WalletAction::Claim { amount } => {
            ensure_unlocked(config_dir)?;
            let claimed = claim_to_wallet(config_dir, amount).await?;
            format!("Claimed {claimed:.6} GRID")
        }
        WalletAction::Send { to, amount, memo } => {
            ensure_unlocked(config_dir)?;
            let sent = send(config_dir, &to, amount, memo).await?;
            let message = format!("Sent {:.6} GRID", sent.amount);
            transaction = Some(sent);
            message
        }
        WalletAction::CreateSolana => {
            ensure_unlocked(config_dir)?;
            let address = crate::solana_wallet::create(config_dir)?;
            format!("Solana reward wallet created: {address}")
        }
        WalletAction::ImportSolana { address } => {
            let address = crate::solana_wallet::import_address(config_dir, &address)?;
            format!("Solana reward address configured: {address}")
        }
    };
    Ok(ActionResult {
        ok: true,
        message,
        recovery_phrase: phrase,
        transaction,
        snapshot: snapshot(config_dir).await,
    })
}

fn ensure_unlocked(config_dir: &Path) -> Result<()> {
    let status = auth_status(config_dir);
    if !status.session_unlocked {
        bail!("vault is locked; unlock it in the wallet first");
    }
    Ok(())
}
