//! Job protocol types shared by coordinator, node, and CLI.

use serde::{Deserialize, Serialize};

/// MVP allowlisted job kinds only.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JobKind {
    Echo,
    HashFile,
}

impl JobKind {
    pub fn parse(s: &str) -> anyhow::Result<Self> {
        match s {
            "echo" => Ok(Self::Echo),
            "hash_file" => Ok(Self::HashFile),
            other => anyhow::bail!("unknown job kind '{other}' (allowlist: echo, hash_file)"),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Echo => "echo",
            Self::HashFile => "hash_file",
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
}

fn default_timeout() -> u64 {
    60
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
