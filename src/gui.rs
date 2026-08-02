//! Stable local JSON contract shared by the native wallet applications.
//! Secret-bearing requests arrive over stdin; snapshots never include secrets.

use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::Duration;

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
    pub network: NetworkSnapshot,
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

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NetworkSnapshot {
    pub mode: String,
    pub truth_url: String,
    pub p2p_peer: String,
    pub connected: bool,
    pub trusted: bool,
    pub chain_id: Option<String>,
    pub height: Option<u64>,
    pub leader_pubkey: Option<String>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct WalletNetworkSettings {
    mode: String,
    truth_url: String,
    p2p_peer: String,
}

impl Default for WalletNetworkSettings {
    fn default() -> Self {
        Self {
            mode: "genesis".into(),
            truth_url: crate::genesis::CANONICAL_TRUTH_URL.into(),
            p2p_peer: crate::genesis::CANONICAL_P2P_PEER.into(),
        }
    }
}

impl WalletNetworkSettings {
    fn path_in(config_dir: &Path) -> PathBuf {
        config_dir.join("wallet-network.json")
    }

    fn load(config_dir: &Path) -> Self {
        std::fs::read_to_string(Self::path_in(config_dir))
            .ok()
            .and_then(|raw| serde_json::from_str(&raw).ok())
            .unwrap_or_default()
    }

    fn save(&self, config_dir: &Path) -> Result<()> {
        validate_network_endpoint(&self.truth_url, &self.p2p_peer)?;
        std::fs::create_dir_all(config_dir)?;
        let path = Self::path_in(config_dir);
        let temp = path.with_extension("json.tmp");
        std::fs::write(&temp, serde_json::to_vec_pretty(self)?)?;
        std::fs::rename(temp, &path)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
        }
        Ok(())
    }
}

#[derive(Debug, Deserialize)]
#[serde(
    tag = "action",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
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
    SetNetwork {
        mode: String,
        truth_url: Option<String>,
        p2p_peer: Option<String>,
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
    let network = network_snapshot(config_dir).await;
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
        network,
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
        WalletAction::SetNetwork {
            mode,
            truth_url,
            p2p_peer,
        } => {
            let settings = match mode.as_str() {
                "genesis" => WalletNetworkSettings::default(),
                "local" => WalletNetworkSettings {
                    mode,
                    truth_url: "http://127.0.0.1:9100".into(),
                    p2p_peer: "127.0.0.1:9900".into(),
                },
                "custom" => WalletNetworkSettings {
                    mode,
                    truth_url: normalize_truth_url(&truth_url.unwrap_or_default()),
                    p2p_peer: p2p_peer.unwrap_or_default().trim().to_string(),
                },
                _ => bail!("network mode must be genesis, local, or custom"),
            };
            settings.save(config_dir)?;
            format!("Wallet network set to {}", settings.mode)
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

async fn network_snapshot(config_dir: &Path) -> NetworkSnapshot {
    let settings = WalletNetworkSettings::load(config_dir);
    let mut snapshot = NetworkSnapshot {
        mode: settings.mode.clone(),
        truth_url: settings.truth_url.clone(),
        p2p_peer: settings.p2p_peer.clone(),
        connected: false,
        trusted: false,
        chain_id: None,
        height: None,
        leader_pubkey: None,
        error: None,
    };
    if let Err(error) = validate_network_endpoint(&settings.truth_url, &settings.p2p_peer) {
        snapshot.error = Some(error.to_string());
        return snapshot;
    }
    let client = match reqwest::Client::builder()
        .timeout(Duration::from_secs(3))
        .build()
    {
        Ok(client) => client,
        Err(error) => {
            snapshot.error = Some(error.to_string());
            return snapshot;
        }
    };
    let response = match client
        .get(format!(
            "{}/health",
            settings.truth_url.trim_end_matches('/')
        ))
        .send()
        .await
    {
        Ok(response) if response.status().is_success() => response,
        Ok(response) => {
            snapshot.error = Some(format!("Genesis health returned {}", response.status()));
            return snapshot;
        }
        Err(error) => {
            snapshot.error = Some(format!("Genesis unavailable: {error}"));
            return snapshot;
        }
    };
    let body: serde_json::Value = match response.json().await {
        Ok(body) => body,
        Err(error) => {
            snapshot.error = Some(format!("Invalid Genesis response: {error}"));
            return snapshot;
        }
    };
    snapshot.connected = body.get("ok").and_then(|value| value.as_bool()) == Some(true);
    snapshot.chain_id = body
        .pointer("/chain/id")
        .and_then(|value| value.as_str())
        .map(str::to_string);
    snapshot.height = body
        .pointer("/chain/height")
        .and_then(|value| value.as_u64());
    snapshot.leader_pubkey = body
        .pointer("/chain/leaderPubkey")
        .and_then(|value| value.as_str())
        .map(str::to_string);
    if settings.mode != "genesis" {
        let role = body.get("role").and_then(|value| value.as_str());
        let advertised_peer = body.pointer("/p2p/listen").and_then(|value| value.as_str());
        if role != Some("node") || advertised_peer.is_none() {
            snapshot.error =
                Some("Endpoint is healthy, but it is not a GRID P2P node wallet API".into());
            snapshot.connected = false;
            return snapshot;
        }
        if peer_port(advertised_peer.unwrap()) != peer_port(&settings.p2p_peer) {
            snapshot.error = Some(format!(
                "Node advertises P2P {}, but Phoenix is configured for {}",
                advertised_peer.unwrap(),
                settings.p2p_peer
            ));
            snapshot.connected = false;
            return snapshot;
        }
    }
    snapshot.trusted = if settings.mode == "genesis" {
        snapshot.leader_pubkey.as_deref() == Some(crate::genesis::CANONICAL_LEADER_PUBKEY)
    } else {
        snapshot.connected
    };
    if snapshot.connected && !snapshot.trusted {
        snapshot.error = Some(
            "Genesis responded, but its leader key does not match the pinned GRID authority".into(),
        );
    }
    snapshot
}

fn validate_network_endpoint(truth_url: &str, p2p_peer: &str) -> Result<()> {
    let url = reqwest::Url::parse(truth_url)?;
    if !matches!(url.scheme(), "http" | "https")
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
    {
        bail!("Genesis truth endpoint must be an http(s) URL without credentials");
    }
    if peer_port(p2p_peer).is_none() {
        bail!("P2P peer must be host:port");
    }
    Ok(())
}

fn normalize_truth_url(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.starts_with("http://") || trimmed.starts_with("https://") || trimmed.is_empty() {
        trimmed.to_string()
    } else {
        format!("http://{trimmed}")
    }
}

fn peer_port(peer: &str) -> Option<u16> {
    let normalized = peer.trim().trim_start_matches("tcp://");
    if let Ok(address) = normalized.parse::<std::net::SocketAddr>() {
        return Some(address.port());
    }
    let (host, port) = normalized.rsplit_once(':')?;
    if host.trim().is_empty() {
        return None;
    }
    port.parse().ok()
}

fn ensure_unlocked(config_dir: &Path) -> Result<()> {
    let status = auth_status(config_dir);
    if !status.session_unlocked {
        bail!("vault is locked; unlock it in the wallet first");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn custom_node_url_adds_http_base() {
        assert_eq!(
            normalize_truth_url("127.0.0.1:9100"),
            "http://127.0.0.1:9100"
        );
        assert_eq!(
            normalize_truth_url(" node.example:9100 "),
            "http://node.example:9100"
        );
    }

    #[test]
    fn custom_node_url_preserves_explicit_scheme() {
        assert_eq!(
            normalize_truth_url("https://node.example"),
            "https://node.example"
        );
    }

    #[test]
    fn wallet_action_accepts_camel_case_custom_node_fields() {
        let action: WalletAction = serde_json::from_value(serde_json::json!({
            "action": "setNetwork",
            "mode": "custom",
            "truthUrl": "127.0.0.1:9100",
            "p2pPeer": "127.0.0.1:9900"
        }))
        .expect("Phoenix custom-node action should deserialize");

        match action {
            WalletAction::SetNetwork {
                mode,
                truth_url,
                p2p_peer,
            } => {
                assert_eq!(mode, "custom");
                assert_eq!(truth_url.as_deref(), Some("127.0.0.1:9100"));
                assert_eq!(p2p_peer.as_deref(), Some("127.0.0.1:9900"));
            }
            _ => panic!("expected setNetwork action"),
        }
    }
}
