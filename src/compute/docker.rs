//! containerd/nerdctl CLI backend — isolated capacity + one-shot job runs.

use anyhow::{bail, Context, Result};
use std::process::Command;
use std::time::Duration;
use tokio::process::Command as AsyncCommand;

use super::isolation::containerd_isolation_args;
use super::manifest::ComputeManifest;
use super::serve::ContainerJobSpec;

#[derive(Debug)]
pub struct ContainerdError(pub String);

impl std::fmt::Display for ContainerdError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}
impl std::error::Error for ContainerdError {}

#[cfg(target_os = "windows")]
const DEFAULT_NERDCTL: &str = "nerdctl.exe";
#[cfg(not(target_os = "windows"))]
const DEFAULT_NERDCTL: &str = "nerdctl";

/// Command is configurable for a local wrapper, notably macOS Lima. The
/// executable name only (not shell text) is accepted to prevent injection.
fn nerdctl_bin() -> String {
    std::env::var("GRID_NERDCTL_BIN")
        .ok()
        .filter(|s| !s.trim().is_empty() && !s.contains(['/', '\\', ' ', '\t', '\n']))
        .unwrap_or_else(|| DEFAULT_NERDCTL.into())
}

pub async fn containerd_available() -> bool {
    AsyncCommand::new(nerdctl_bin())
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
    let pull = AsyncCommand::new(nerdctl_bin())
        .args(["pull", &m.image])
        .output()
        .await
        .context("containerd/nerdctl pull")?;
    if !pull.status.success() {
        // Image may already be local
        let inspect = AsyncCommand::new(nerdctl_bin())
            .args(["image", "inspect", &m.image])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .await
            .context("containerd/nerdctl image inspect")?;
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
    let _ = Command::new(nerdctl_bin()).args(["stop", id]).status();
    Ok(())
}

pub fn rm_container(id: &str) -> Result<()> {
    let _ = Command::new(nerdctl_bin()).args(["rm", "-f", id]).status();
    Ok(())
}

pub fn logs(id: &str, follow: bool) -> Result<()> {
    let mut cmd = Command::new(nerdctl_bin());
    cmd.arg("logs");
    if follow {
        cmd.arg("-f");
    }
    cmd.arg(id);
    let st = cmd.status().context("containerd/nerdctl logs")?;
    if !st.success() {
        bail!("containerd/nerdctl logs failed");
    }
    Ok(())
}

/// One-shot isolated job container (host path).
pub async fn run_job(spec: &ContainerJobSpec) -> Result<(bool, String, u64)> {
    let t0 = std::time::Instant::now();
    let cname = format!("grid-job-{}", &uuid::Uuid::new_v4().to_string()[..8]);
    let timeout = Duration::from_secs(spec.timeout_sec.max(5));

    let mut args = vec![
        "run".into(),
        "--rm".into(),
        "--name".into(),
        cname.clone(),
        "--label".into(),
        "grid.job=1".into(),
    ];
    args.extend(containerd_isolation_args(
        spec.cpus,
        spec.memory_mb,
        spec.network,
    ));
    if let Some(port) = spec.service_port {
        // This is deliberately loopback-only. containerd/nerdctl never exposes the job
        // container on a host/LAN/WAN interface, and the container receives no
        // containerd/nerdctl socket, mounts, elevated caps, or host namespaces.
        args.push("--publish".into());
        args.push(format!("127.0.0.1:{port}:{port}"));
        args.push("--label".into());
        args.push(format!("grid.service.port={port}"));
    }
    for (k, v) in &spec.env {
        args.push("-e".into());
        args.push(format!("{k}={v}"));
    }
    args.push(spec.image.clone());
    for c in &spec.cmd {
        args.push(c.clone());
    }

    let child = AsyncCommand::new(nerdctl_bin())
        .args(&args)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .context("spawn containerd/nerdctl run")?;

    let output = match tokio::time::timeout(timeout, child.wait_with_output()).await {
        Ok(Ok(o)) => o,
        Ok(Err(e)) => {
            let _ = Command::new(nerdctl_bin())
                .args(["rm", "-f", &cname])
                .status();
            bail!("containerd/nerdctl wait: {e}");
        }
        Err(_) => {
            let _ = Command::new(nerdctl_bin())
                .args(["rm", "-f", &cname])
                .status();
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
