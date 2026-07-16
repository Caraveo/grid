//! Allowlisted local job execution (Phase 1 — no Docker yet).

use crate::protocol::JobKind;
use std::time::Instant;

#[derive(Debug, Clone)]
pub struct ExecResult {
    pub ok: bool,
    pub output: String,
    pub duration_ms: u64,
    pub error: Option<String>,
}

pub fn execute(kind: JobKind, payload: &str) -> ExecResult {
    let t0 = Instant::now();
    match kind {
        JobKind::Echo => ExecResult {
            ok: true,
            output: payload.to_string(),
            duration_ms: t0.elapsed().as_millis() as u64,
            error: None,
        },
        JobKind::HashFile => ExecResult {
            ok: true,
            output: crate::crypto::sha256_hex(payload.as_bytes()),
            duration_ms: t0.elapsed().as_millis() as u64,
            error: None,
        },
    }
}

/// Deterministic re-check for coordinator verification v0.
pub fn expected_output(kind: JobKind, payload: &str) -> String {
    match kind {
        JobKind::Echo => payload.to_string(),
        JobKind::HashFile => crate::crypto::sha256_hex(payload.as_bytes()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn echo_and_hash() {
        let e = execute(JobKind::Echo, "hi");
        assert!(e.ok && e.output == "hi");
        let h = execute(JobKind::HashFile, "hi");
        assert_eq!(h.output, expected_output(JobKind::HashFile, "hi"));
        assert_eq!(h.output.len(), 64);
    }
}
