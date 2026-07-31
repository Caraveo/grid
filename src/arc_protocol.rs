//! Canonical Arc wallet envelopes.
//!
//! Clients sign these bytes locally. Genesis verifies the signature, address,
//! chain, expiry, and monotonically increasing account nonce before mutation.

use anyhow::{bail, Context, Result};
use chrono::Utc;
use serde::{Deserialize, Serialize};

use crate::address::{encode_payment_hex, normalize_address};
use crate::passkey::verify_operator_sig;

const SEND_DOMAIN: &str = "GRID-ARC-SEND-v1";
pub const MAX_MEMO_BYTES: usize = 280;
pub const MAX_ENVELOPE_LIFETIME_SECS: i64 = 10 * 60;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArcSendIntent {
    pub version: u32,
    pub chain_id: String,
    pub from: String,
    pub to: String,
    /// Fixed decimal text. Never sign a binary float representation.
    pub amount: String,
    pub memo: String,
    pub nonce: u64,
    pub expires_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SignedArcSend {
    pub intent: ArcSendIntent,
    pub public_key: String,
    pub signature: String,
}

pub fn send_signing_bytes(intent: &ArcSendIntent) -> Result<Vec<u8>> {
    let mut bytes = SEND_DOMAIN.as_bytes().to_vec();
    bytes.push(b'\n');
    bytes.extend(serde_json::to_vec(intent)?);
    Ok(bytes)
}

pub fn validate_signed_send(
    envelope: &SignedArcSend,
    expected_chain_id: &str,
    expected_nonce: u64,
) -> Result<f64> {
    let intent = &envelope.intent;
    if intent.version != 1 {
        bail!("unsupported transaction version");
    }
    if intent.chain_id != expected_chain_id {
        bail!("transaction is for a different chain");
    }
    let from = normalize_address(&intent.from)?;
    let to = normalize_address(&intent.to)?;
    if from == to {
        bail!("cannot send to self");
    }
    if encode_payment_hex(&envelope.public_key)? != from {
        bail!("public key does not derive the sender address");
    }
    if intent.nonce != expected_nonce {
        bail!("invalid nonce: expected {expected_nonce}");
    }
    let now = Utc::now().timestamp();
    if intent.expires_at < now {
        bail!("transaction expired");
    }
    if intent.expires_at > now + MAX_ENVELOPE_LIFETIME_SECS {
        bail!("transaction expiry is too far in the future");
    }
    if intent.memo.as_bytes().len() > MAX_MEMO_BYTES {
        bail!("memo exceeds {MAX_MEMO_BYTES} bytes");
    }
    if intent.amount.contains(['e', 'E']) {
        bail!("amount must use fixed decimal notation");
    }
    let amount: f64 = intent.amount.parse().context("invalid amount")?;
    if amount <= 0.0 || !amount.is_finite() {
        bail!("amount must be positive");
    }
    let decimals = intent
        .amount
        .split_once('.')
        .map(|(_, fraction)| fraction.len())
        .unwrap_or(0);
    if decimals > 12 {
        bail!("amount supports at most 12 decimal places");
    }
    verify_operator_sig(
        &envelope.public_key,
        &send_signing_bytes(intent)?,
        &envelope.signature,
    )?;
    let _ = to;
    Ok(amount)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::{Signer, SigningKey};

    fn signed(nonce: u64) -> SignedArcSend {
        let secret = SigningKey::from_bytes(&[7u8; 32]);
        let public_key = hex::encode(secret.verifying_key().as_bytes());
        let from = encode_payment_hex(&public_key).unwrap();
        let to_key = SigningKey::from_bytes(&[9u8; 32]);
        let to = encode_payment_hex(&hex::encode(to_key.verifying_key().as_bytes())).unwrap();
        let intent = ArcSendIntent {
            version: 1,
            chain_id: "grid-test".into(),
            from,
            to,
            amount: "1.250000000000".into(),
            memo: "test".into(),
            nonce,
            expires_at: Utc::now().timestamp() + 120,
        };
        let signature = hex::encode(
            secret
                .sign(&send_signing_bytes(&intent).unwrap())
                .to_bytes(),
        );
        SignedArcSend {
            intent,
            public_key,
            signature,
        }
    }

    #[test]
    fn accepts_canonical_signed_send() {
        validate_signed_send(&signed(1), "grid-test", 1).unwrap();
    }

    #[test]
    fn rejects_replay_nonce() {
        let error = validate_signed_send(&signed(1), "grid-test", 2).unwrap_err();
        assert!(error.to_string().contains("nonce"));
    }

    #[test]
    fn rejects_tampered_amount() {
        let mut envelope = signed(1);
        envelope.intent.amount = "500.0".into();
        assert!(validate_signed_send(&envelope, "grid-test", 1).is_err());
    }
}
