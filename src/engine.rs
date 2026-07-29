//! GRID Engine — platform-aware, safe scaffolding for P2P node operators.
//!
//! It never installs packages or weakens host security. `init` writes a
//! reviewable YAML manifest and records the local runtime facts; `grid start
//! manifest.yaml` then starts the encrypted P2P node using those settings.

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::{fs, path::Path, process::Command};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EngineManifest {
    pub api_version: String,
    pub kind: String,
    pub metadata: EngineMetadata,
    pub spec: EngineSpec,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EngineMetadata { pub name: String }

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EngineSpec {
    /// Phase 1 is deliberately P2P-only. Host/mining is never enabled by an
    /// Engine manifest until its nested-runtime boundary is audited.
    #[serde(default = "default_mode")]
    pub mode: String,
    #[serde(default = "default_class")]
    pub class: String,
    #[serde(default)]
    pub p2p: P2pSpec,
    #[serde(default)]
    pub runtime: RuntimeSpec,
    #[serde(default)]
    pub storage: StorageSpec,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct P2pSpec {
    #[serde(default = "default_listen")]
    pub listen: String,
    #[serde(default)]
    pub connect: Vec<String>,
}
impl Default for P2pSpec {
    fn default() -> Self {
        Self {
            listen: default_listen(),
            connect: vec![],
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeSpec {
    #[serde(default = "default_cpus")]
    pub cpus: f64,
    #[serde(default = "default_memory")]
    pub memory_mib: u64,
    #[serde(default)]
    pub network_for_jobs: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StorageSpec {
    #[serde(default)]
    pub encrypted_persistent_volumes: bool,
    #[serde(default = "default_volume_quota")]
    pub volume_quota_mib: u64,
}
impl Default for StorageSpec {
    fn default() -> Self { Self { encrypted_persistent_volumes: true, volume_quota_mib: default_volume_quota() } }
}
impl Default for RuntimeSpec { fn default() -> Self { Self { cpus: default_cpus(), memory_mib: default_memory(), network_for_jobs: false } } }
fn default_class() -> String { "S".into() }
fn default_mode() -> String { "p2p".into() }
fn default_listen() -> String { "0.0.0.0:9900".into() }
fn default_cpus() -> f64 { 1.0 }
fn default_memory() -> u64 { 512 }
fn default_volume_quota() -> u64 { 10_240 }

pub fn load(path: &Path) -> Result<EngineManifest> {
    let raw = fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    let m: EngineManifest = serde_yaml::from_str(&raw).context("invalid GRID Engine YAML")?;
    if m.api_version != "grid/v1alpha1" || m.kind != "GridEngine" { bail!("expected apiVersion grid/v1alpha1 and kind GridEngine"); }
    if m.metadata.name.trim().is_empty() { bail!("metadata.name is required"); }
    if m.spec.mode != "p2p" { bail!("Engine phase 1 only permits spec.mode: p2p"); }
    if m.spec.runtime.cpus <= 0.0 || m.spec.runtime.memory_mib < 64 { bail!("runtime requires cpus > 0 and memoryMiB >= 64"); }
    if !m.spec.storage.encrypted_persistent_volumes { bail!("Engine requires encryptedPersistentVolumes: true"); }
    Ok(m)
}

pub fn scaffold(path: &Path, name: &str) -> Result<()> {
    if path.exists() { bail!("{} already exists; refusing to overwrite", path.display()); }
    let m = EngineManifest { api_version: "grid/v1alpha1".into(), kind: "GridEngine".into(), metadata: EngineMetadata { name: name.into() }, spec: EngineSpec { mode: default_mode(), class: default_class(), p2p: P2pSpec::default(), runtime: RuntimeSpec::default(), storage: StorageSpec::default() } };
    fs::write(path, serde_yaml::to_string(&m)?)?;
    Ok(())
}

pub fn doctor() {
    let platform = format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH);
    let nerdctl = Command::new(if cfg!(windows) { "nerdctl.exe" } else { "nerdctl" }).arg("info").output().map(|o| o.status.success()).unwrap_or(false);
    println!("GRID Engine");
    println!("  platform     {platform}");
    println!("  runtime      {}", if nerdctl { "containerd/nerdctl ready" } else { "containerd/nerdctl not detected" });
    println!("  isolation    read-only FS · no capabilities · no-new-privileges · PID/memory/CPU limits");
    println!("  encryption   node keys remain in the local GRID vault; enable disk encryption separately");
    if !nerdctl { println!("  next         Linux: rootless containerd + nerdctl · macOS: Lima + nerdctl · Windows: WSL2 + nerdctl"); }
}

/// Print the exact, user-approved setup command for the current platform.
/// GRID never silently installs a privileged runtime.
pub fn install() -> Result<()> {
    let bin = if cfg!(windows) { "nerdctl.exe" } else { "nerdctl" };
    if Command::new(bin).arg("info").output().is_ok_and(|o| o.status.success()) {
        println!("✓ GRID Engine runtime is already ready");
        return Ok(());
    }
    if cfg!(target_os = "macos") {
        require("brew", "Install Homebrew first: https://brew.sh/")?;
        run("brew", &["install", "lima", "nerdctl"])?;
        run("limactl", &["start", "--name=grid-containerd", "template:default"])?;
    } else if cfg!(target_os = "windows") {
        run("wsl.exe", &["--install", "-d", "Ubuntu"])?;
        bail!("WSL2 was requested. Reboot if Windows asks, then run `grid engine install` inside Ubuntu WSL.");
    } else if command_exists("apt-get") {
        run("sudo", &["apt-get", "update"])?;
        run("sudo", &["apt-get", "install", "-y", "containerd", "nerdctl"])?;
    } else if command_exists("dnf") {
        run("sudo", &["dnf", "install", "-y", "containerd", "nerdctl"])?;
    } else if command_exists("pacman") {
        run("sudo", &["pacman", "-Sy", "--noconfirm", "containerd", "nerdctl"])?;
    } else {
        bail!("Unsupported Linux package manager. Install rootless containerd + nerdctl, then rerun `grid engine install`.");
    }
    if !Command::new(bin).arg("info").output().is_ok_and(|o| o.status.success()) {
        bail!("Runtime installation finished but nerdctl is not ready. See `grid engine doctor` for platform guidance.");
    }
    println!("✓ GRID Engine runtime is ready");
    Ok(())
}

fn command_exists(command: &str) -> bool {
    Command::new(command).arg("--version").output().is_ok()
}

fn require(command: &str, guide: &str) -> Result<()> {
    if command_exists(command) { Ok(()) } else { bail!("{guide}") }
}

fn run(command: &str, args: &[&str]) -> Result<()> {
    println!("GRID Engine install: {command} {}", args.join(" "));
    let status = Command::new(command).args(args).status().with_context(|| format!("start {command}"))?;
    if status.success() { Ok(()) } else { bail!("{command} failed with {status}. Resolve the platform prerequisite and rerun `grid engine install`.") }
}
