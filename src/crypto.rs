//! Hashing helpers (local receipts). Bitcoin remains the Transact Security Layer.

use sha2::{Digest, Sha256};

pub fn sha256_hex(data: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(data);
    hex::encode(h.finalize())
}

pub fn blake3_hex(data: &[u8]) -> String {
    hex::encode(blake3::hash(data).as_bytes())
}

/// Digest intended for later Bitcoin commitment anchoring (Phase 3+).
pub fn commitment_digest(data: &[u8]) -> String {
    blake3_hex(data)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sha256_stable() {
        assert!(!sha256_hex(b"hello-grid").is_empty());
        assert_eq!(sha256_hex(b"a").len(), 64);
        assert_eq!(sha256_hex(b"a"), sha256_hex(b"a"));
    }
}
