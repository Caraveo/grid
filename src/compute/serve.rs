//! Serve one host-track container job.

use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};

use super::allowlist::is_image_allowed;
use super::docker::{containerd_available, run_job};
use super::tunnel::{validate_container_port, GRID_CONTAINER_PORT};
use crate::executor::ExecResult;
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
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
    /// Permit the assigned launcher to reach this job container through the
    /// GRID encrypted data plane. This never grants host access.
    #[serde(default)]
    pub tunnel: bool,
    /// The only permitted in-container service port. It is published on host
    /// loopback only while the one-shot job is alive.
    #[serde(default)]
    pub service_port: Option<u16>,
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
            s.validate_tunnel()?;
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
            tunnel: false,
            service_port: None,
            compute: None,
            env: vec![],
        })
    }

    pub fn validate_tunnel(&self) -> Result<()> {
        match (self.tunnel, self.service_port) {
            (false, None) => Ok(()),
            (false, Some(_)) => bail!("servicePort requires tunnel=true"),
            (true, Some(port)) => {
                validate_container_port(port)?;
                if !self.network {
                    bail!("tunnel=true requires the isolated Docker bridge network");
                }
                Ok(())
            }
            (true, None) => bail!("tunnel=true requires servicePort={GRID_CONTAINER_PORT}"),
        }
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

    if !containerd_available().await {
        return ExecResult {
            ok: false,
            output: String::new(),
            duration_ms: 0,
            error: Some("containerd/nerdctl not available — start containerd with nerdctl".into()),
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
    if !containerd_available().await {
        return Err("docker required to verify container_work".into());
    }
    match run_job(&spec).await {
        Ok((true, out, _)) => Ok(out),
        Ok((false, out, _)) => Err(format!("verify container failed: {out}")),
        Err(e) => Err(e.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tunnel_requires_the_one_grid_container_port() {
        let ok = ContainerJobSpec::parse(
            r#"{"image":"alpine:3.20","cmd":["echo","ok"],"network":true,"tunnel":true,"servicePort":41783}"#,
        );
        assert!(ok.is_ok());
        let arbitrary = ContainerJobSpec::parse(
            r#"{"image":"alpine:3.20","network":true,"tunnel":true,"servicePort":8080}"#,
        );
        assert!(arbitrary.is_err());
        let no_network = ContainerJobSpec::parse(
            r#"{"image":"alpine:3.20","tunnel":true,"servicePort":41783}"#,
        );
        assert!(no_network.is_err());
    }
}
