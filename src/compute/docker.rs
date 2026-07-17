//! Docker CLI backend — isolated capacity + one-shot job runs.

use anyhow::{bail, Context, Result};
use std::process::Command;
use std::time::Duration;
use tokio::process::Command as AsyncCommand;

use super::isolation::docker_isolation_args;
use super::manifest::ComputeManifest;
use super::serve::ContainerJobSpec;

#[derive(Debug)]
pub struct DockerError(pub String);

impl std::fmt::Display for DockerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}
impl std::error::Error for DockerError {}

pub async fn docker_available() -> bool {
    AsyncCommand::new("docker")
        .args(["info"])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .await
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Register capacity: pull allowlisted image. Jobs are one-shot isolated runs
/// (no long-lived host-mounted containers).
pub async fn ensure_capacity(m: &ComputeManifest) -> Result<Vec<String>> {
    let pull = AsyncCommand::new("docker")
        .args(["pull", &m.image])
        .output()
        .await
        .context("docker pull")?;
    if !pull.status.success() {
        // Image may already be local
        let inspect = AsyncCommand::new("docker")
            .args(["image", "inspect", &m.image])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .await
            .context("docker image inspect")?;
        if !inspect.success() {
            bail!(
                "cannot pull or find image {}: {}",
                m.image,
                String::from_utf8_lossy(&pull.stderr)
            );
        }
    }
    // No idle containers — free slots = replicas in manifest/status.
    Ok(vec![])
}

pub fn stop_container(id: &str) -> Result<()> {
    let _ = Command::new("docker").args(["stop", id]).status();
    Ok(())
}

pub fn rm_container(id: &str) -> Result<()> {
    let _ = Command::new("docker").args(["rm", "-f", id]).status();
    Ok(())
}

pub fn logs(id: &str, follow: bool) -> Result<()> {
    let mut cmd = Command::new("docker");
    cmd.arg("logs");
    if follow {
        cmd.arg("-f");
    }
    cmd.arg(id);
    let st = cmd.status().context("docker logs")?;
    if !st.success() {
        bail!("docker logs failed");
    }
    Ok(())
}

/// One-shot isolated job container (host path).
pub async fn run_job(spec: &ContainerJobSpec) -> Result<(bool, String, u64)> {
    let t0 = std::time::Instant::now();
    let cname = format!(
        "grid-job-{}",
        &uuid::Uuid::new_v4().to_string()[..8]
    );
    let timeout = Duration::from_secs(spec.timeout_sec.max(5));

    let mut args = vec![
        "run".into(),
        "--rm".into(),
        "--name".into(),
        cname.clone(),
        "--label".into(),
        "grid.job=1".into(),
    ];
    args.extend(docker_isolation_args(spec.cpus, spec.memory_mb, spec.network));
    for (k, v) in &spec.env {
        args.push("-e".into());
        args.push(format!("{k}={v}"));
    }
    args.push(spec.image.clone());
    for c in &spec.cmd {
        args.push(c.clone());
    }

    let child = AsyncCommand::new("docker")
        .args(&args)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .context("spawn docker run")?;

    let output = match tokio::time::timeout(timeout, child.wait_with_output()).await {
        Ok(Ok(o)) => o,
        Ok(Err(e)) => {
            let _ = Command::new("docker").args(["rm", "-f", &cname]).status();
            bail!("docker wait: {e}");
        }
        Err(_) => {
            let _ = Command::new("docker").args(["rm", "-f", &cname]).status();
            return Ok((
                false,
                format!("timeout after {}s", spec.timeout_sec),
                t0.elapsed().as_millis() as u64,
            ));
        }
    };

    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    let ms = t0.elapsed().as_millis() as u64;
    if output.status.success() {
        Ok((true, stdout, ms))
    } else {
        let msg = if stderr.is_empty() {
            stdout
        } else if stdout.is_empty() {
            stderr
        } else {
            format!("{stdout}\n{stderr}")
        };
        Ok((false, msg, ms))
    }
}
