//! GRID native addresses — Bech32 with HRP `grid0`.
//!
//! Format: `grid0` + `1` + data+checksum  → always starts with **`grid01`**.
//! (Bech32 requires the separator `1` after the human-readable part.)
//!
//! v0 payload: 20-byte BLAKE3 hash of the 32-byte Ed25519 public key
//!   → everyday payment address (`grid01q…`)
//!
//! Distinct from:
//! - `grid://name.grid`  (human realm)
//! - `gp://{128-hex}`    (wire identity)
//! - `bc1…`              (Bitcoin TSL exit)

use anyhow::{bail, Context, Result};
use blake3::Hasher;

/// Human-readable part — yields addresses starting with `grid01`.
pub const HRP: &str = "grid0";

/// Witness-style version for standard payment addresses.
pub const VERSION_PAYMENT: u8 = 0;

const CHARSET: &[u8] = b"qpzry9x8gf2tvdw0s3jn54khce6mua7l";

fn polymod(values: &[u8]) -> u32 {
    const GEN: [u32; 5] = [0x3b6a57b2, 0x26508e6d, 0x1ea119fa, 0x3d4233dd, 0x2a1462b3];
    let mut chk: u32 = 1;
    for &v in values {
        let b = chk >> 25;
        chk = ((chk & 0x1ffffff) << 5) ^ u32::from(v);
        for (i, g) in GEN.iter().enumerate() {
            if ((b >> i) & 1) == 1 {
                chk ^= g;
            }
        }
    }
    chk
}

fn hrp_expand(hrp: &str) -> Vec<u8> {
    let mut out = Vec::with_capacity(hrp.len() * 2 + 1);
    for b in hrp.bytes() {
        out.push(b >> 5);
    }
    out.push(0);
    for b in hrp.bytes() {
        out.push(b & 31);
    }
    out
}

fn create_checksum(hrp: &str, data: &[u8]) -> Vec<u8> {
    let mut values = hrp_expand(hrp);
    values.extend_from_slice(data);
    values.extend_from_slice(&[0, 0, 0, 0, 0, 0]);
    let p = polymod(&values) ^ 1;
    (0..6).map(|i| ((p >> (5 * (5 - i))) & 31) as u8).collect()
}

fn encode(hrp: &str, data: &[u8]) -> String {
    let checksum = create_checksum(hrp, data);
    let mut out = String::with_capacity(hrp.len() + 1 + data.len() + 6);
    out.push_str(hrp);
    out.push('1');
    for &d in data.iter().chain(checksum.iter()) {
        out.push(CHARSET[d as usize] as char);
    }
    out
}

fn convertbits(data: &[u8], from: u32, to: u32, pad: bool) -> Result<Vec<u8>> {
    let mut acc: u32 = 0;
    let mut bits: u32 = 0;
    let mut ret = Vec::new();
    let maxv = (1u32 << to) - 1;
    for &value in data {
        if u32::from(value) >> from != 0 {
            bail!("invalid convertbits input");
        }
        acc = (acc << from) | u32::from(value);
        bits += from;
        while bits >= to {
            bits -= to;
            ret.push(((acc >> bits) & maxv) as u8);
        }
    }
    if pad {
        if bits != 0 {
            ret.push(((acc << (to - bits)) & maxv) as u8);
        }
    } else if bits >= from || ((acc << (to - bits)) & maxv) != 0 {
        bail!("invalid convertbits padding");
    }
    Ok(ret)
}

fn charset_find(c: u8) -> Option<u8> {
    CHARSET.iter().position(|&x| x == c).map(|i| i as u8)
}

fn decode_raw(s: &str) -> Result<(String, Vec<u8>)> {
    let s = s.trim();
    if s.len() < 8 {
        bail!("address too short");
    }
    // mixed case forbidden
    let lower = s.to_ascii_lowercase();
    let upper = s.to_ascii_uppercase();
    if s != lower.as_str() && s != upper.as_str() {
        bail!("mixed-case bech32 not allowed");
    }
    let s = lower;
    let pos = s.rfind('1').context("missing bech32 separator '1'")?;
    if pos < 1 || pos + 7 > s.len() {
        bail!("invalid bech32 layout");
    }
    let hrp = &s[..pos];
    if hrp != HRP {
        bail!("expected HRP '{HRP}', got '{hrp}'");
    }
    let data_part = &s[pos + 1..];
    let mut data = Vec::with_capacity(data_part.len());
    for b in data_part.bytes() {
        let v = charset_find(b).context("invalid bech32 character")?;
        data.push(v);
    }
    if data.len() < 6 {
        bail!("checksum too short");
    }
    let mut values = hrp_expand(hrp);
    values.extend_from_slice(&data);
    if polymod(&values) != 1 {
        bail!("invalid bech32 checksum");
    }
    data.truncate(data.len() - 6);
    Ok((hrp.to_string(), data))
}

/// HASH160-style: first 20 bytes of BLAKE3(pubkey32).
pub fn hash20_pubkey(pubkey32: &[u8]) -> Result<[u8; 20]> {
    if pubkey32.len() != 32 {
        bail!("pubkey must be 32 bytes");
    }
    let mut h = Hasher::new();
    h.update(b"GRID0-ADDR-v0");
    h.update(pubkey32);
    let dig = h.finalize();
    let mut out = [0u8; 20];
    out.copy_from_slice(&dig.as_bytes()[..20]);
    Ok(out)
}

/// Encode a payment address from a 32-byte Ed25519 public key.
pub fn encode_payment(pubkey32: &[u8]) -> Result<String> {
    let h20 = hash20_pubkey(pubkey32)?;
    let mut data = vec![VERSION_PAYMENT];
    data.extend(convertbits(&h20, 8, 5, true)?);
    let addr = encode(HRP, &data);
    if !addr.starts_with("grid0") {
        bail!("internal: address must start with grid0");
    }
    Ok(addr)
}

/// Encode from hex pubkey (64 hex chars).
pub fn encode_payment_hex(pubkey_hex: &str) -> Result<String> {
    let bytes = hex::decode(pubkey_hex.trim()).context("decode pubkey hex")?;
    encode_payment(&bytes)
}

/// Validate and parse a `grid0…` address. Returns (version, payload bytes).
pub fn decode_address(addr: &str) -> Result<(u8, Vec<u8>)> {
    let (_hrp, data5) = decode_raw(addr)?;
    if data5.is_empty() {
        bail!("empty address data");
    }
    let version = data5[0];
    let payload = convertbits(&data5[1..], 5, 8, false)?;
    if version == VERSION_PAYMENT && payload.len() != 20 {
        bail!("payment address payload must be 20 bytes");
    }
    Ok((version, payload))
}

pub fn is_valid_address(addr: &str) -> bool {
    decode_address(addr).is_ok()
}

/// Normalize address to lowercase if valid.
pub fn normalize_address(addr: &str) -> Result<String> {
    let a = addr.trim().to_ascii_lowercase();
    decode_address(&a)?;
    Ok(a)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_and_prefix() {
        let pk = [0x42u8; 32];
        let addr = encode_payment(&pk).unwrap();
        assert!(addr.starts_with("grid01"), "got {addr}");
        assert!(is_valid_address(&addr));
        let (v, payload) = decode_address(&addr).unwrap();
        assert_eq!(v, 0);
        assert_eq!(payload.len(), 20);
        assert_eq!(payload, hash20_pubkey(&pk).unwrap());
    }

    #[test]
    fn rejects_bad_checksum() {
        let pk = [0x11u8; 32];
        let mut addr = encode_payment(&pk).unwrap();
        // flip last char
        let last = addr.pop().unwrap();
        let flipped = if last == 'q' { 'p' } else { 'q' };
        addr.push(flipped);
        assert!(decode_address(&addr).is_err());
    }

    #[test]
    fn rejects_wrong_hrp() {
        assert!(decode_address("bc1qw508d6qejxtdg4y5r3zarvary0c5xw7kv8f3t4").is_err());
    }
}
