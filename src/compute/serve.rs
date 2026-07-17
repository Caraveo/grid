//! Serve one host-track container job.

use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};

use super::allowlist::is_image_allowed;
use super::docker::{docker_available, run_job};
use crate::executor::ExecResult;
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContainerJobSpec {
    pub image: String,
    #[serde(default)]
    pub cmd: Vec<String>,
    #[serde(default = "default_timeout")]
    pub timeout_sec: u64,
    #[serde(default = "default_cpus")]
    pub cpus: f64,
    #[serde(default = "default_mem")]
    pub memory_mb: u64,
    #[serde(default)]
    pub network: bool,
    #[serde(default)]
    pub compute: Option<String>,
    #[serde(default)]
    pub env: Vec<(String, String)>,
}

fn default_timeout() -> u64 {
    120
}
fn default_cpus() -> f64 {
    0.5
}
fn default_mem() -> u64 {
    256
}

impl ContainerJobSpec {
    pub fn parse(payload: &str) -> Result<Self> {
        let t = payload.trim();
        if t.starts_with('{') {
            let mut s: Self = serde_json::from_str(t)?;
            if s.cmd.is_empty() {
                s.cmd = vec!["echo".into(), "grid-host-ok".into()];
            }
            return Ok(s);
        }
        // shorthand: image|arg1|arg2…
        let parts: Vec<&str> = t.split('|').collect();
        if parts.is_empty() || parts[0].is_empty() {
            bail!("container_work payload: JSON or image|cmd…");
        }
        Ok(Self {
            image: parts[0].to_string(),
            cmd: if parts.len() > 1 {
                parts[1..].iter().map(|s| s.to_string()).collect()
            } else {
                vec!["echo".into(), "grid-host-ok".into()]
            },
            timeout_sec: 120,
            cpus: 0.5,
            memory_mb: 256,
            network: false,
            compute: None,
            env: vec![],
        })
    }
}

/// Run allowlisted isolated container job for host path.
pub async fn serve_container_job(config_dir: &Path, payload: &str) -> ExecResult {
    let spec = match ContainerJobSpec::parse(payload) {
        Ok(s) => s,
        Err(e) => {
            return ExecResult {
                ok: false,
                output: String::new(),
                duration_ms: 0,
                error: Some(e.to_string()),
            };
        }
    };

    match is_image_allowed(config_dir, &spec.image) {
        Ok(true) => {}
        Ok(false) => {
            return ExecResult {
                ok: false,
                output: String::new(),
                duration_ms: 0,
                error: Some(format!("image not allowlisted: {}", spec.image)),
            };
        }
        Err(e) => {
            return ExecResult {
                ok: false,
                output: String::new(),
                duration_ms: 0,
                error: Some(e.to_string()),
            };
        }
    }

    if !docker_available().await {
        return ExecResult {
            ok: false,
            output: String::new(),
            duration_ms: 0,
            error: Some("Docker not available — start Colima/Docker Desktop".into()),
        };
    }

    match run_job(&spec).await {
        Ok((ok, output, ms)) => ExecResult {
            ok,
            output,
            duration_ms: ms,
            error: if ok {
                None
            } else {
                Some("container exit non-zero".into())
            },
        },
        Err(e) => ExecResult {
            ok: false,
            output: String::new(),
            duration_ms: 0,
            error: Some(e.to_string()),
        },
    }
}

/// Deterministic expected output for simple echo-style host jobs (coord verify).
#[allow(dead_code)]
pub async fn expected_container_output(payload: &str) -> Result<String, String> {
    let spec = ContainerJobSpec::parse(payload).map_err(|e| e.to_string())?;
    // For alpine/busybox echo patterns, predict without docker when possible
    if spec.cmd.len() >= 2 && (spec.cmd[0] == "echo" || spec.cmd[0].ends_with("/echo")) {
        return Ok(spec.cmd[1..].join(" "));
    }
    if !docker_available().await {
        return Err("docker required to verify container_work".into());
    }
    match run_job(&spec).await {
        Ok((true, out, _)) => Ok(out),
        Ok((false, out, _)) => Err(format!("verify container failed: {out}")),
        Err(e) => Err(e.to_string()),
    }
}
