//! One-time, end-to-end encrypted pairing from a local GRID vault to ARK.
//!
//! The request contains only a mobile ephemeral X25519 public key. The response
//! encrypts the wallet signing key directly to that key with XChaCha20-Poly1305.
//! Genesis is never involved and never sees wallet secret material.

use anyhow::{bail, Context, Result};
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{ChaCha20Poly1305, Nonce};
use chrono::Utc;
use hkdf::Hkdf;
use rand::rngs::OsRng;
use rand::RngCore;
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use std::path::Path;
use x25519_dalek::{EphemeralSecret, PublicKey};
use zeroize::Zeroizing;

use crate::passkey::{load_operator_signing_key, require_identity};
use crate::wallet::WalletMeta;

const REQUEST_PREFIX: &str = "gridark://pair/v1/request?data=";
const RESPONSE_PREFIX: &str = "gridark://pair/v1/response?data=";
const PAIR_INFO: &[u8] = b"GRID-ARK-PAIR-v1";
const LEGACY_REQUEST_PREFIX: &str = "gridarc://pair/v1/request?data=";
const LEGACY_RESPONSE_PREFIX: &str = "gridarc://pair/v1/response?data=";
const LEGACY_PAIR_INFO: &[u8] = b"GRID-ARC-PAIR-v1";
const MAX_PAIRING_LIFETIME_SECS: i64 = 5 * 60;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PairingRequest {
    pub version: u32,
    pub request_id: String,
    pub device_name: String,
    pub device_public_key: String,
    pub expires_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PairingResponse {
    pub version: u32,
    pub request_id: String,
    pub sender_public_key: String,
    pub nonce: String,
    pub ciphertext: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct WalletTransfer<'a> {
    version: u32,
    address: &'a str,
    public_key: &'a str,
    secret_key: &'a str,
    created_at: i64,
}

pub async fn create_pairing_response(config_dir: &Path, request_uri: &str) -> Result<String> {
    let trimmed = request_uri.trim();
    let (encoded, response_prefix, pair_info) =
        if let Some(encoded) = trimmed.strip_prefix(REQUEST_PREFIX) {
            (encoded, RESPONSE_PREFIX, PAIR_INFO)
        } else if let Some(encoded) = trimmed.strip_prefix(LEGACY_REQUEST_PREFIX) {
            (encoded, LEGACY_RESPONSE_PREFIX, LEGACY_PAIR_INFO)
        } else {
            bail!("expected an ARK pairing request")
        };
    let request_bytes = URL_SAFE_NO_PAD
        .decode(encoded)
        .context("invalid pairing request encoding")?;
    if request_bytes.len() > 4_096 {
        bail!("pairing request is too large");
    }
    let request: PairingRequest =
        serde_json::from_slice(&request_bytes).context("invalid pairing request")?;
    let now = Utc::now().timestamp();
    if request.version != 1
        || request.request_id.len() < 16
        || request.device_name.as_bytes().len() > 80
        || request.expires_at < now
        || request.expires_at > now + MAX_PAIRING_LIFETIME_SECS
    {
        bail!("invalid or expired pairing request");
    }
    let mobile_key: [u8; 32] = hex::decode(&request.device_public_key)?
        .as_slice()
        .try_into()
        .map_err(|_| anyhow::anyhow!("device public key must be 32 bytes"))?;

    // Require the strongest locally configured identity gate immediately before
    // decrypting the existing wallet key.
    let dek = require_identity(config_dir, "Pair existing wallet with ARK").await?;
    let signing = load_operator_signing_key(config_dir, &dek)?;
    let wallet = WalletMeta::load(config_dir)?;
    if signing.verifying_key().as_bytes() != hex::decode(&wallet.pubkey_hex)?.as_slice() {
        bail!("wallet metadata does not match the unlocked operator key");
    }

    let ephemeral = EphemeralSecret::random_from_rng(OsRng);
    let sender_public = PublicKey::from(&ephemeral);
    let shared = ephemeral.diffie_hellman(&PublicKey::from(mobile_key));
    let hkdf = Hkdf::<Sha256>::new(Some(request.request_id.as_bytes()), shared.as_bytes());
    let mut encryption_key = [0u8; 32];
    hkdf.expand(pair_info, &mut encryption_key)
        .map_err(|_| anyhow::anyhow!("pairing key derivation failed"))?;

    let secret_hex = Zeroizing::new(hex::encode(signing.to_bytes()));
    let transfer = WalletTransfer {
        version: 1,
        address: &wallet.address,
        public_key: &wallet.pubkey_hex,
        secret_key: secret_hex.as_str(),
        created_at: now,
    };
    let plaintext = Zeroizing::new(serde_json::to_vec(&transfer)?);
    let mut nonce = [0u8; 12];
    OsRng.fill_bytes(&mut nonce);
    let ciphertext = ChaCha20Poly1305::new((&encryption_key).into())
        .encrypt(
            Nonce::from_slice(&nonce),
            Payload {
                msg: plaintext.as_slice(),
                aad: &request_bytes,
            },
        )
        .map_err(|_| anyhow::anyhow!("pairing encryption failed"))?;
    encryption_key.fill(0);

    let response = PairingResponse {
        version: 1,
        request_id: request.request_id,
        sender_public_key: hex::encode(sender_public.as_bytes()),
        nonce: URL_SAFE_NO_PAD.encode(nonce),
        ciphertext: URL_SAFE_NO_PAD.encode(ciphertext),
    };
    Ok(format!(
        "{response_prefix}{}",
        URL_SAFE_NO_PAD.encode(serde_json::to_vec(&response)?)
    ))
}
