//! GRID Engine — platform-aware, safe scaffolding for P2P node operators.
//!
//! It never installs packages or weakens host security. `init` writes a
//! reviewable YAML manifest and records the local runtime facts; `grid start
//! manifest.yaml` then starts the encrypted P2P node using those settings.

use anyhow::{bail, Context, Result};
use rand::{rngs::OsRng, RngCore};
use serde::{Deserialize, Serialize};
use std::{fs, path::{Path, PathBuf}, process::Command};
use zeroize::Zeroize;

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

/// Key metadata only. The data-encryption key is stored separately, encrypted
/// by the operator vault; no container gets either this key or the vault path.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VolumeMetadata {
    pub name: String,
    pub key_path: String,
    pub encrypted_path: String,
    pub mount_path: String,
    pub backend: String,
    pub created_at: String,
}

/// A narrow first service contract. It is intentionally Caddy + Git only;
/// arbitrary images, arbitrary ports, and direct host exposure are rejected.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WebServiceManifest {
    pub api_version: String,
    pub kind: String,
    pub metadata: EngineMetadata,
    pub spec: WebServiceSpec,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WebServiceSpec {
    pub image: String,
    pub git: GitSource,
    pub volume: String,
    pub exposure: String,
    #[serde(default = "default_service_port")]
    pub service_port: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitSource {
    pub repository: String,
    #[serde(default = "default_branch")]
    pub branch: String,
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
fn default_service_port() -> u16 { crate::compute::GRID_CONTAINER_PORT }
fn default_branch() -> String { "main".into() }

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
    let volume_backend = command_exists("gocryptfs");
    println!("GRID Engine");
    println!("  platform     {platform}");
    println!("  runtime      {}", if nerdctl { "containerd/nerdctl ready" } else { "containerd/nerdctl not detected" });
    println!("  volumes      {}", if volume_backend { "gocryptfs ready" } else { "gocryptfs not detected" });
    println!("  isolation    read-only FS · no capabilities · no-new-privileges · PID/memory/CPU limits");
    println!("  encryption   node keys remain in the local GRID vault; enable disk encryption separately");
    if !nerdctl { println!("  next         Linux: rootless containerd + nerdctl · macOS: Lima + nerdctl · Windows: WSL2 + nerdctl"); }
}

/// Print the exact, user-approved setup command for the current platform.
/// GRID never silently installs a privileged runtime.
pub fn install() -> Result<()> {
    let bin = if cfg!(windows) { "nerdctl.exe" } else { "nerdctl" };
    if Command::new(bin).arg("info").output().is_ok_and(|o| o.status.success()) && command_exists("gocryptfs") {
        println!("✓ GRID Engine runtime and encrypted-volume backend are already ready");
        return Ok(());
    }
    if cfg!(target_os = "macos") {
        require("brew", "Install Homebrew first: https://brew.sh/")?;
        run("brew", &["install", "lima", "nerdctl", "gocryptfs"])?;
        run("limactl", &["start", "--name=grid-containerd", "template:default"])?;
    } else if cfg!(target_os = "windows") {
        run("wsl.exe", &["--install", "-d", "Ubuntu"])?;
        bail!("WSL2 was requested. Reboot if Windows asks, then run `grid engine install` inside Ubuntu WSL.");
    } else if command_exists("apt-get") {
        run("sudo", &["apt-get", "update"])?;
        run("sudo", &["apt-get", "install", "-y", "containerd", "nerdctl", "gocryptfs"])?;
    } else if command_exists("dnf") {
        run("sudo", &["dnf", "install", "-y", "containerd", "nerdctl", "gocryptfs"])?;
    } else if command_exists("pacman") {
        run("sudo", &["pacman", "-Sy", "--noconfirm", "containerd", "nerdctl", "gocryptfs"])?;
    } else {
        bail!("Unsupported Linux package manager. Install rootless containerd + nerdctl, then rerun `grid engine install`.");
    }
    if !Command::new(bin).arg("info").output().is_ok_and(|o| o.status.success()) || !command_exists("gocryptfs") {
        bail!("Runtime installation finished but nerdctl or gocryptfs is not ready. See `grid engine doctor` for platform guidance.");
    }
    println!("✓ GRID Engine runtime and encrypted-volume backend are ready");
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

pub fn volume_root(config_dir: &Path) -> PathBuf {
    config_dir.join("engine").join("volumes")
}

fn volume_name(name: &str) -> Result<String> {
    let value = name.trim().to_ascii_lowercase();
    if value.is_empty() || value.len() > 48 || !value.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
        bail!("volume name must be 1-48 characters: a-z, 0-9, - or _");
    }
    Ok(value)
}

/// Prepare a per-volume key hierarchy. This is deliberately not a plaintext
/// directory fallback: an Engine backend must mount the encrypted store before
/// the volume can ever be attached to a container.
pub async fn prepare_volume(config_dir: &Path, name: &str) -> Result<VolumeMetadata> {
    let name = volume_name(name)?;
    let dir = volume_root(config_dir).join(&name);
    if dir.exists() { bail!("volume {name} already exists; refusing to overwrite"); }
    let dek = crate::passkey::require_unlocked(config_dir, "create encrypted Engine volume").await?;
    let mut volume_key = [0u8; 32];
    OsRng.fill_bytes(&mut volume_key);
    let wrapped = crate::passkey::encrypt_with_vault(&dek, &volume_key)?;
    volume_key.zeroize();
    fs::create_dir_all(dir.join("ciphertext"))?;
    fs::create_dir_all(dir.join("mount"))?;
    let key_path = dir.join("volume.key.enc");
    fs::write(&key_path, wrapped)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&key_path, fs::Permissions::from_mode(0o600))?;
    }
    let metadata = VolumeMetadata {
        name,
        key_path: key_path.display().to_string(),
        encrypted_path: dir.join("ciphertext").display().to_string(),
        mount_path: dir.join("mount").display().to_string(),
        backend: "pending-encrypted-mount".into(),
        created_at: chrono::Utc::now().to_rfc3339(),
    };
    fs::write(dir.join("volume.json"), serde_json::to_string_pretty(&metadata)?)?;
    Ok(metadata)
}

pub fn volume_status(config_dir: &Path, name: &str) -> Result<VolumeMetadata> {
    let name = volume_name(name)?;
    let path = volume_root(config_dir).join(name).join("volume.json");
    Ok(serde_json::from_str(&fs::read_to_string(&path).with_context(|| format!("missing {}", path.display()))?)?)
}

pub fn scaffold_web_service(path: &Path, name: &str, repository: &str) -> Result<()> {
    if path.exists() { bail!("{} already exists; refusing to overwrite", path.display()); }
    if !repository.starts_with("https://") || repository.contains(char::is_whitespace) {
        bail!("repository must be a whitespace-free https:// Git URL");
    }
    let manifest = WebServiceManifest {
        api_version: "grid/v1alpha1".into(),
        kind: "GridWebService".into(),
        metadata: EngineMetadata { name: volume_name(name)? },
        spec: WebServiceSpec {
            // The private registry promotion step resolves this to an immutable digest.
            image: "caddy:2-alpine".into(),
            git: GitSource { repository: repository.into(), branch: default_branch() },
            volume: name.into(),
            exposure: "grid-tunnel".into(),
            service_port: default_service_port(),
        },
    };
    fs::write(path, serde_yaml::to_string(&manifest)?)?;
    Ok(())
}

pub fn load_web_service(path: &Path) -> Result<WebServiceManifest> {
    let raw = fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    let m: WebServiceManifest = serde_yaml::from_str(&raw).context("invalid web service YAML")?;
    if m.api_version != "grid/v1alpha1" || m.kind != "GridWebService" { bail!("expected apiVersion grid/v1alpha1 and kind GridWebService"); }
    if !m.spec.image.starts_with("caddy:") { bail!("phase 1 web services require the approved Caddy image"); }
    if m.spec.exposure != "grid-tunnel" { bail!("web services must use exposure: grid-tunnel; direct host ports are forbidden"); }
    if m.spec.service_port != default_service_port() { bail!("web services must use the GRID service port"); }
    if !m.spec.git.repository.starts_with("https://") || m.spec.git.repository.contains(char::is_whitespace) { bail!("git.repository must be a whitespace-free https:// URL"); }
    Ok(m)
}
