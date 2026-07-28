use anyhow::{bail, Context, Result};
use ed25519_dalek::SigningKey;
use rand::rngs::OsRng;
use serde_json::Value;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::Path;

use crate::config::NodeConfig;

pub const GRID_DEVNET_MINT: &str = "XkQDUHx2kgrGZ2SKaJeCZ2UyZicMESiNybjTNtPvbXK";
const DEVNET_RPC: &str = "https://api.devnet.solana.com";

pub fn validate_address(address: &str) -> Result<String> {
    let trimmed = address.trim();
    let bytes = bs58::decode(trimmed)
        .into_vec()
        .context("Solana address is not valid base58")?;
    if bytes.len() != 32 {
        bail!("Solana address must decode to 32 bytes");
    }
    Ok(trimmed.to_string())
}

fn save_address(config_dir: &Path, address: &str) -> Result<()> {
    let path = NodeConfig::path_in(config_dir);
    let mut config = NodeConfig::load(&path)
        .with_context(|| "initialize the node first: grid init --name my-node --class S")?;
    config.solana_reward_wallet = Some(address.to_string());
    config.save(&path)
}

pub fn create(config_dir: &Path) -> Result<String> {
    let key_dir = config_dir.join("keys");
    std::fs::create_dir_all(&key_dir)?;
    let path = key_dir.join("solana-reward.json");
    if path.exists() {
        bail!(
            "{} already exists; refusing to overwrite a reward key",
            path.display()
        );
    }
    let signing = SigningKey::generate(&mut OsRng);
    let verifying = signing.verifying_key();
    let address = bs58::encode(verifying.as_bytes()).into_string();
    let mut secret = Vec::with_capacity(64);
    secret.extend_from_slice(&signing.to_bytes());
    secret.extend_from_slice(verifying.as_bytes());
    let encoded = serde_json::to_vec(&secret)?;
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(&path)?;
    file.write_all(&encoded)?;
    file.sync_all()?;
    save_address(config_dir, &address)?;
    Ok(address)
}

pub fn import_address(config_dir: &Path, address: &str) -> Result<String> {
    let address = validate_address(address)?;
    save_address(config_dir, &address)?;
    Ok(address)
}

pub async fn balance(config_dir: &Path) -> Result<(String, f64, bool)> {
    let config = NodeConfig::load(&NodeConfig::path_in(config_dir))
        .with_context(|| "initialize the node first: grid init --name my-node --class S")?;
    let Some(address) = config.solana_reward_wallet else {
        bail!("Solana reward wallet is not configured");
    };
    validate_address(&address)?;
    let body = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "getTokenAccountsByOwner",
        "params": [
            address,
            { "mint": GRID_DEVNET_MINT },
            { "encoding": "jsonParsed", "commitment": "confirmed" }
        ]
    });
    let response: Value = reqwest::Client::new()
        .post(DEVNET_RPC)
        .json(&body)
        .send()
        .await
        .context("contact Solana devnet")?
        .error_for_status()
        .context("Solana devnet rejected the request")?
        .json()
        .await?;
    if let Some(error) = response.get("error") {
        bail!("Solana RPC error: {error}");
    }
    let accounts = response["result"]["value"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let balance = accounts
        .iter()
        .filter_map(|account| {
            account["account"]["data"]["parsed"]["info"]["tokenAmount"]["uiAmountString"]
                .as_str()
                .and_then(|value| value.parse::<f64>().ok())
        })
        .sum::<f64>();
    Ok((
        address,
        balance,
        config_dir.join("keys").join("solana-reward.json").exists(),
    ))
}

pub async fn status(config_dir: &Path) -> Result<()> {
    let (address, balance, local_key) = match balance(config_dir).await {
        Ok(value) => value,
        Err(error) if error.to_string().contains("not configured") => {
            println!("Solana reward wallet: not configured");
            println!("  grid solana create");
            println!("  grid solana import <ADDRESS>");
            return Ok(());
        }
        Err(error) => return Err(error),
    };
    println!("Solana reward wallet");
    println!("  address   {address}");
    println!("  network   devnet");
    println!("  GRID      {balance:.6}");
    println!("  mint      {GRID_DEVNET_MINT}");
    println!("  explorer  https://explorer.solana.com/address/{address}?cluster=devnet");
    println!(
        "  custody   {}",
        if local_key {
            config_dir
                .join("keys")
                .join("solana-reward.json")
                .display()
                .to_string()
        } else {
            "external / watch-only".to_string()
        }
    );
    Ok(())
}
