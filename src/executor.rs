//! Allowlisted verifiable job execution (Phase 1 pilot — real CPU PoR).

use crate::protocol::JobKind;
use std::time::Instant;

#[derive(Debug, Clone)]
pub struct ExecResult {
    pub ok: bool,
    pub output: String,
    pub duration_ms: u64,
    pub error: Option<String>,
}

/// Default PoR iterations when payload omits count.
pub const DEFAULT_BLAKE3_ITERS: u64 = 250_000;
/// Hard cap so a bad payload cannot hang a node forever.
pub const MAX_BLAKE3_ITERS: u64 = 5_000_000;

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
        JobKind::Blake3Work => match blake3_work(payload) {
            Ok(out) => ExecResult {
                ok: true,
                output: out,
                duration_ms: t0.elapsed().as_millis() as u64,
                error: None,
            },
            Err(e) => ExecResult {
                ok: false,
                output: String::new(),
                duration_ms: t0.elapsed().as_millis() as u64,
                error: Some(e),
            },
        },
        // Container jobs must use async host path (`compute::serve_container_job`).
        JobKind::ContainerWork => ExecResult {
            ok: false,
            output: String::new(),
            duration_ms: t0.elapsed().as_millis() as u64,
            error: Some("container_work requires grid host (async docker path)".into()),
        },
    }
}

/// Deterministic re-check for coordinator verification (sync kinds only).
pub fn expected_output(kind: JobKind, payload: &str) -> Result<String, String> {
    match kind {
        JobKind::Echo => Ok(payload.to_string()),
        JobKind::HashFile => Ok(crate::crypto::sha256_hex(payload.as_bytes())),
        JobKind::Blake3Work => blake3_work(payload),
        JobKind::ContainerWork => {
            // Prefer pure prediction for echo-style cmds; coord async path may re-run docker.
            if let Ok(spec) = crate::compute::ContainerJobSpec::parse(payload) {
                if spec.cmd.len() >= 2 && (spec.cmd[0] == "echo" || spec.cmd[0].ends_with("/echo"))
                {
                    return Ok(spec.cmd[1..].join(" "));
                }
            }
            Err("container_work: use async expected_container_output on coordinator".into())
        }
    }
}

/// Parse `seed|iters` (iters optional) and run iterated BLAKE3.
///
/// Algorithm (stable — do not change without bumping kind version):
///   h0 = BLAKE3(seed)
///   h_{i+1} = BLAKE3(h_i)   for i in 0..iters-1  (on raw 32-byte digests after first)
///   output = hex(h_{iters-1})  if iters>=1, else hex(h0) with iters forced >=1
fn blake3_work(payload: &str) -> Result<String, String> {
    let (seed, iters) = parse_blake3_payload(payload)?;
    Ok(run_blake3_chain(&seed, iters))
}

pub fn parse_blake3_payload(payload: &str) -> Result<(String, u64), String> {
    let payload = payload.trim();
    if payload.is_empty() {
        return Err("blake3_work payload empty (want seed|iterations)".into());
    }
    let (seed, iters) = if let Some((s, n)) = payload.rsplit_once('|') {
        let n = n.trim().parse::<u64>().map_err(|_| format!("bad iterations: {n}"))?;
        (s.trim().to_string(), n)
    } else {
        (payload.to_string(), DEFAULT_BLAKE3_ITERS)
    };
    if seed.is_empty() {
        return Err("blake3_work seed empty".into());
    }
    if seed.len() > 256 {
        return Err("blake3_work seed too long (max 256)".into());
    }
    if iters == 0 {
        return Err("blake3_work iterations must be >= 1".into());
    }
    if iters > MAX_BLAKE3_ITERS {
        return Err(format!(
            "blake3_work iterations {iters} exceeds max {MAX_BLAKE3_ITERS}"
        ));
    }
    Ok((seed, iters))
}

pub fn run_blake3_chain(seed: &str, iters: u64) -> String {
    // First hash is over seed bytes; subsequent hashes over previous 32-byte digest.
    let mut dig = *blake3::hash(seed.as_bytes()).as_bytes();
    let mut i = 1u64;
    while i < iters {
        dig = *blake3::hash(&dig).as_bytes();
        i += 1;
    }
    hex::encode(dig)
}

/// Build a fabric auto-work payload (unique seed, fixed effort class).
pub fn fabric_work_payload(epoch_unix: u64, seq: u64, iters: u64) -> String {
    format!("grid-por-v1:{epoch_unix}:{seq}|{iters}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blake3_stable_and_verified() {
        let payload = "hello|1000";
        let a = execute(JobKind::Blake3Work, payload);
        assert!(a.ok, "{:?}", a.error);
        assert_eq!(a.output.len(), 64);
        assert_eq!(
            a.output,
            expected_output(JobKind::Blake3Work, payload).unwrap()
        );
        // different seed → different digest
        let b = execute(JobKind::Blake3Work, "other|1000");
        assert_ne!(a.output, b.output);
    }

    #[test]
    fn hash_file() {
        let h = execute(JobKind::HashFile, "hi");
        assert_eq!(h.output, expected_output(JobKind::HashFile, "hi").unwrap());
        assert_eq!(h.output.len(), 64);
    }
}
