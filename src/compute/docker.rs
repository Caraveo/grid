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

fn use_lima_nerdctl() -> bool {
    cfg!(target_os = "macos")
        && std::env::var("GRID_NERDCTL_BIN")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .is_none()
}

fn nerdctl_command() -> Command {
    let configured = nerdctl_bin();
    if use_lima_nerdctl() {
        let mut command = Command::new("limactl");
        command.args(["shell", "grid-containerd", "nerdctl"]);
        command
    } else {
        Command::new(configured)
    }
}

fn nerdctl_async_command() -> AsyncCommand {
    let configured = nerdctl_bin();
    if use_lima_nerdctl() {
        let mut command = AsyncCommand::new("limactl");
        command.args(["shell", "grid-containerd", "nerdctl"]);
        command
    } else {
        AsyncCommand::new(configured)
    }
}

pub async fn containerd_available() -> bool {
    nerdctl_async_command()
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
    let pull = nerdctl_async_command()
        .args(["pull", &m.image])
        .output()
        .await
        .context("containerd/nerdctl pull")?;
    if !pull.status.success() {
        // Image may already be local
        let inspect = nerdctl_async_command()
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
    let _ = nerdctl_command().args(["stop", id]).status();
    Ok(())
}

pub fn rm_container(id: &str) -> Result<()> {
    let _ = nerdctl_command().args(["rm", "-f", id]).status();
    Ok(())
}

pub fn logs(id: &str, follow: bool) -> Result<()> {
    let mut cmd = nerdctl_command();
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

/// Start the one approved long-lived Engine workload. The source tree is
/// recursively read-only inside Caddy, the service binds to host loopback only,
/// and no vault/key/runtime socket is mounted.
pub async fn run_caddy_service(
    name: &str,
    image: &str,
    source: &std::path::Path,
    port: u16,
) -> Result<(String, String)> {
    if image != "caddy:2-alpine" {
        bail!("Engine phase 1 only permits caddy:2-alpine");
    }
    super::tunnel::validate_container_port(port)?;
    let cname = format!("grid-engine-{name}");
    let pull = nerdctl_async_command()
        .args(["pull", image])
        .output()
        .await
        .context("pull approved Caddy image")?;
    if !pull.status.success() {
        bail!("cannot pull {image}: {}", String::from_utf8_lossy(&pull.stderr));
    }
    let inspect = nerdctl_async_command()
        .args([
            "image",
            "inspect",
            "--format",
            "{{index .RepoDigests 0}}",
            image,
        ])
        .output()
        .await
        .context("resolve approved Caddy image digest")?;
    if !inspect.status.success() {
        bail!("cannot resolve immutable digest for {image}");
    }
    let image_digest = String::from_utf8(inspect.stdout)?.trim().to_string();
    let digest_hex = image_digest
        .strip_prefix("caddy@sha256:")
        .ok_or_else(|| anyhow::anyhow!("container runtime returned an unexpected Caddy digest"))?;
    if digest_hex.len() != 64 || !digest_hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        bail!("container runtime returned an invalid Caddy sha256 digest");
    }
    let _ = nerdctl_command()
        .args(["rm", "-f", &cname])
        .status();
    let source = source
        .canonicalize()
        .with_context(|| format!("canonicalize {}", source.display()))?;
    let args = vec![
        "run".to_string(),
        "-d".to_string(),
        "--name".to_string(),
        cname.clone(),
        "--label".to_string(),
        format!("grid.engine.service={name}"),
        "--read-only".to_string(),
        "--tmpfs".to_string(),
        "/tmp:rw,noexec,nosuid,size=32m".to_string(),
        "--cap-drop".to_string(),
        "ALL".to_string(),
        // The official Caddy image marks its binary with this file capability.
        // Linux refuses to exec it if the capability is absent from the bounding
        // set, even though Engine serves only on a high unprivileged port.
        "--cap-add".to_string(),
        "NET_BIND_SERVICE".to_string(),
        "--security-opt".to_string(),
        "no-new-privileges".to_string(),
        "--pids-limit".to_string(),
        "128".to_string(),
        "--memory".to_string(),
        "512m".to_string(),
        "--cpus".to_string(),
        "1".to_string(),
        "--user".to_string(),
        "65534:65534".to_string(),
        "--network".to_string(),
        "bridge".to_string(),
        "--mount".to_string(),
        format!(
            "type=bind,source={},target=/srv,readonly,bind-propagation=rprivate",
            source.display()
        ),
        "--publish".to_string(),
        format!("127.0.0.1:{port}:{port}"),
        image_digest.clone(),
        "caddy".to_string(),
        "file-server".to_string(),
        "--root".to_string(),
        "/srv".to_string(),
        "--listen".to_string(),
        format!(":{port}"),
    ];
    let output = nerdctl_async_command()
        .args(&args)
        .output()
        .await
        .context("start isolated Caddy service")?;
    if !output.status.success() {
        bail!(
            "Caddy service failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    Ok((cname, image_digest))
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

    let child = nerdctl_async_command()
        .args(&args)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .context("spawn containerd/nerdctl run")?;

    let output = match tokio::time::timeout(timeout, child.wait_with_output()).await {
        Ok(Ok(o)) => o,
        Ok(Err(e)) => {
            let _ = nerdctl_command()
                .args(["rm", "-f", &cname])
                .status();
            bail!("containerd/nerdctl wait: {e}");
        }
        Err(_) => {
            let _ = nerdctl_command()
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
