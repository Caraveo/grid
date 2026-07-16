//! Job protocol types shared by coordinator, node, and CLI.

use serde::{Deserialize, Serialize};

/// Allowlisted job kinds — only deterministic, verifiable work.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JobKind {
    /// Legacy identity job (kept for compatibility; not used by fabric auto-work).
    Echo,
    /// SHA-256 of payload bytes (content digest).
    HashFile,
    /// CPU Proof-of-Resource: iterated BLAKE3 from seed (verifiable).
    /// Payload: `seed|iterations` e.g. `grid-por-v1|250000`
    Blake3Work,
}

impl JobKind {
    pub fn parse(s: &str) -> anyhow::Result<Self> {
        match s.trim().to_lowercase().as_str() {
            "echo" => Ok(Self::Echo),
            "hash_file" | "hash" | "sha256" => Ok(Self::HashFile),
            "blake3_work" | "blake3" | "por" | "work" => Ok(Self::Blake3Work),
            other => anyhow::bail!(
                "unknown job kind '{other}' (allowlist: blake3_work, hash_file, echo)"
            ),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Echo => "echo",
            Self::HashFile => "hash_file",
            Self::Blake3Work => "blake3_work",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Job {
    pub id: String,
    pub kind: String,
    pub payload: String,
    pub created_at: String,
    #[serde(default = "default_timeout")]
    pub timeout_sec: u64,
    #[serde(default)]
    pub status: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub assigned_node_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub earn_credits: Option<f64>,
    /// Result commitment (sha256 of canonical result) when verified.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result_commitment: Option<String>,
    /// Operator pubkey that submitted the result (hex), if provided.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub operator_pubkey: Option<String>,
}

fn default_timeout() -> u64 {
    120
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JobResult {
    pub node_id: String,
    pub ok: bool,
    pub output: String,
    pub duration_ms: u64,
    pub commitment: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NodeInfo {
    pub node_id: String,
    pub class: String,
    #[serde(default)]
    pub gpu_model: String,
    #[serde(default = "one")]
    pub max_concurrent: u32,
    #[serde(default)]
    pub cluster_id: String,
    #[serde(default)]
    pub last_seen: i64,
    #[serde(default)]
    pub jobs_done: u64,
    #[serde(default)]
    pub jobs_failed: u64,
    #[serde(default)]
    pub earn_total: f64,
    #[serde(default)]
    pub label: String,
}

fn one() -> u32 {
    1
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompleteResponse {
    pub job: Job,
    pub verified: bool,
    pub earn_credits: f64,
}

/// Canonical commitment for a result (verifier v0).
pub fn result_commitment(
    job_id: &str,
    node_id: &str,
    ok: bool,
    output: &str,
    duration_ms: u64,
) -> String {
    let flag = if ok { "1" } else { "0" };
    let canonical = format!("{job_id}|{node_id}|{flag}|{output}|{duration_ms}");
    crate::crypto::sha256_hex(canonical.as_bytes())
}
