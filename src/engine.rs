//! GRID Engine — platform-aware, safe scaffolding for P2P node operators.
//!
//! It never installs packages or weakens host security. `init` writes a
//! reviewable YAML manifest and records the local runtime facts; `grid start
//! manifest.yaml` then starts the encrypted P2P node using those settings.

use anyhow::{bail, Context, Result};
use rand::{rngs::OsRng, RngCore};
use serde::{Deserialize, Serialize};
use sha2::Digest;
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};
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
pub struct EngineMetadata {
    pub name: String,
}

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
pub struct ServiceDeployKey {
    pub service: String,
    pub repository: String,
    pub public_key: String,
    pub private_key_path: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct TunnelCapabilityVerifier {
    service: String,
    capability_sha256: String,
    client_pubkey: String,
    issued_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServiceRuntime {
    pub service: String,
    pub state: String,
    pub container: String,
    pub locator: String,
    pub repository: String,
    pub commit: String,
    pub image: String,
    pub image_digest: String,
    pub volume: String,
    pub loopback_port: u16,
    pub public_exposure: bool,
    pub started_at: String,
    pub operator_pubkey: String,
    pub receipt_signature: String,
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
    fn default() -> Self {
        Self {
            encrypted_persistent_volumes: true,
            volume_quota_mib: default_volume_quota(),
        }
    }
}
impl Default for RuntimeSpec {
    fn default() -> Self {
        Self {
            cpus: default_cpus(),
            memory_mib: default_memory(),
            network_for_jobs: false,
        }
    }
}
fn default_class() -> String {
    "S".into()
}
fn default_mode() -> String {
    "p2p".into()
}
fn default_listen() -> String {
    "0.0.0.0:9900".into()
}
fn default_cpus() -> f64 {
    1.0
}
fn default_memory() -> u64 {
    512
}
fn default_volume_quota() -> u64 {
    10_240
}
fn default_service_port() -> u16 {
    crate::compute::GRID_CONTAINER_PORT
}
fn default_branch() -> String {
    "main".into()
}

pub fn load(path: &Path) -> Result<EngineManifest> {
    let raw = fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    let m: EngineManifest = serde_yaml::from_str(&raw).context("invalid GRID Engine YAML")?;
    if m.api_version != "grid/v1alpha1" || m.kind != "GridEngine" {
        bail!("expected apiVersion grid/v1alpha1 and kind GridEngine");
    }
    if m.metadata.name.trim().is_empty() {
        bail!("metadata.name is required");
    }
    if m.spec.mode != "p2p" {
        bail!("Engine phase 1 only permits spec.mode: p2p");
    }
    if m.spec.runtime.cpus <= 0.0 || m.spec.runtime.memory_mib < 64 {
        bail!("runtime requires cpus > 0 and memoryMiB >= 64");
    }
    if !m.spec.storage.encrypted_persistent_volumes {
        bail!("Engine requires encryptedPersistentVolumes: true");
    }
    Ok(m)
}

pub fn scaffold(path: &Path, name: &str) -> Result<()> {
    if path.exists() {
        bail!("{} already exists; refusing to overwrite", path.display());
    }
    let m = EngineManifest {
        api_version: "grid/v1alpha1".into(),
        kind: "GridEngine".into(),
        metadata: EngineMetadata { name: name.into() },
        spec: EngineSpec {
            mode: default_mode(),
            class: default_class(),
            p2p: P2pSpec::default(),
            runtime: RuntimeSpec::default(),
            storage: StorageSpec::default(),
        },
    };
    fs::write(path, serde_yaml::to_string(&m)?)?;
    Ok(())
}

pub fn doctor() {
    let platform = format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH);
    let nerdctl = runtime_ready_sync();
    let volume_backend = engine_tool_exists("gocryptfs");
    println!("GRID Engine");
    println!("  platform     {platform}");
    println!(
        "  runtime      {}",
        if nerdctl {
            "containerd/nerdctl ready"
        } else {
            "containerd/nerdctl not detected"
        }
    );
    println!(
        "  volumes      {}",
        if volume_backend {
            "gocryptfs ready"
        } else {
            "gocryptfs not detected"
        }
    );
    println!("  isolation    read-only FS · Caddy bind-service cap only · no-new-privileges · PID/memory/CPU limits");
    println!("  encryption   node keys remain in the local GRID vault; enable disk encryption separately");
    if !nerdctl {
        println!("  next         Linux: rootless containerd + nerdctl · macOS: Lima + nerdctl · Windows: WSL2 + nerdctl");
    }
}

/// Install the supported runtime after the user explicitly invokes
/// `grid engine install`. Containers remain rootless; platform package setup
/// may request the operating system's normal administrator authorization.
pub fn install(config_dir: &Path) -> Result<()> {
    if runtime_ready_sync() && engine_tool_exists("gocryptfs") {
        if cfg!(target_os = "macos") {
            prepare_macos_guest()?;
        }
        println!("✓ GRID Engine runtime and encrypted-volume backend are already ready");
        return Ok(());
    }
    if cfg!(target_os = "macos") {
        require("brew", "Install Homebrew first: https://brew.sh/")?;
        run("brew", &["install", "lima"])?;
        let engine_dir = config_dir.join("engine");
        fs::create_dir_all(&engine_dir)?;
        let engine_mount = format!("{}:w", engine_dir.display());
        let existing = Command::new("limactl")
            .args(["list", "grid-containerd", "--quiet"])
            .output()
            .map(|output| {
                output.status.success()
                    && String::from_utf8_lossy(&output.stdout)
                        .lines()
                        .any(|name| name.trim() == "grid-containerd")
            })
            .unwrap_or(false);
        if existing {
            run("limactl", &["start", "grid-containerd"])?;
        } else {
            run(
                "limactl",
                &[
                    "create",
                    "--name=grid-containerd",
                    "--containerd=user",
                    "--mount-only",
                    &engine_mount,
                    "-y",
                    "template:default",
                ],
            )?;
            run("limactl", &["start", "grid-containerd"])?;
        }
        run(
            "limactl",
            &["shell", "grid-containerd", "sudo", "apt-get", "update"],
        )?;
        run(
            "limactl",
            &[
                "shell",
                "grid-containerd",
                "sudo",
                "apt-get",
                "install",
                "-y",
                "gocryptfs",
                "git",
                "fuse3",
            ],
        )?;
        prepare_macos_guest()?;
    } else if cfg!(target_os = "windows") {
        run("wsl.exe", &["--install", "-d", "Ubuntu"])?;
        bail!("WSL2 was requested. Reboot if Windows asks, then run `grid engine install` inside Ubuntu WSL.");
    } else if command_exists("apt-get") {
        run("sudo", &["apt-get", "update"])?;
        run(
            "sudo",
            &[
                "apt-get",
                "install",
                "-y",
                "containerd",
                "nerdctl",
                "gocryptfs",
            ],
        )?;
    } else if command_exists("dnf") {
        run(
            "sudo",
            &["dnf", "install", "-y", "containerd", "nerdctl", "gocryptfs"],
        )?;
    } else if command_exists("pacman") {
        run(
            "sudo",
            &[
                "pacman",
                "-Sy",
                "--noconfirm",
                "containerd",
                "nerdctl",
                "gocryptfs",
            ],
        )?;
    } else {
        bail!("Unsupported Linux package manager. Install rootless containerd + nerdctl, then rerun `grid engine install`.");
    }
    if !runtime_ready_sync() || !engine_tool_exists("gocryptfs") {
        bail!("Runtime installation finished but nerdctl or gocryptfs is not ready. See `grid engine doctor` for platform guidance.");
    }
    println!("✓ GRID Engine runtime and encrypted-volume backend are ready");
    Ok(())
}

fn prepare_macos_guest() -> Result<()> {
    run(
        "limactl",
        &[
            "shell",
            "grid-containerd",
            "sudo",
            "nerdctl",
            "apparmor",
            "load",
        ],
    )?;
    run(
        "limactl",
        &[
            "shell",
            "grid-containerd",
            "sudo",
            "sysctl",
            "-w",
            "net.ipv4.ip_forward=1",
        ],
    )
}

fn runtime_ready_sync() -> bool {
    if cfg!(target_os = "macos") {
        Command::new("limactl")
            .args(["shell", "grid-containerd", "nerdctl", "info"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|status| status.success())
            .unwrap_or(false)
    } else {
        Command::new(if cfg!(windows) {
            "nerdctl.exe"
        } else {
            "nerdctl"
        })
        .arg("info")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
    }
}

fn command_exists(command: &str) -> bool {
    Command::new(command).arg("--version").output().is_ok()
}

fn engine_tool_exists(command: &str) -> bool {
    if cfg!(target_os = "macos") {
        Command::new("limactl")
            .args(["shell", "grid-containerd", command, "--version"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|status| status.success())
            .unwrap_or(false)
    } else {
        command_exists(command)
    }
}

fn require(command: &str, guide: &str) -> Result<()> {
    if command_exists(command) {
        Ok(())
    } else {
        bail!("{guide}")
    }
}

fn run(command: &str, args: &[&str]) -> Result<()> {
    println!("GRID Engine install: {command} {}", args.join(" "));
    let status = Command::new(command)
        .args(args)
        .status()
        .with_context(|| format!("start {command}"))?;
    if status.success() {
        Ok(())
    } else {
        bail!("{command} failed with {status}. Resolve the platform prerequisite and rerun `grid engine install`.")
    }
}

pub fn volume_root(config_dir: &Path) -> PathBuf {
    config_dir.join("engine").join("volumes")
}

fn volume_mount_path(config_dir: &Path, name: &str) -> PathBuf {
    if cfg!(target_os = "macos") {
        let config_hash = blake3::hash(config_dir.to_string_lossy().as_bytes())
            .to_hex()
            .to_string();
        PathBuf::from("/var/tmp/grid-engine")
            .join(&config_hash[..16])
            .join("mounts")
            .join(name)
    } else {
        volume_root(config_dir).join(name).join("mount")
    }
}

fn volume_name(name: &str) -> Result<String> {
    let value = name.trim().to_ascii_lowercase();
    if value.is_empty()
        || value.len() > 48
        || !value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
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
    if dir.exists() {
        bail!("volume {name} already exists; refusing to overwrite");
    }
    let dek =
        crate::passkey::require_unlocked(config_dir, "create encrypted Engine volume").await?;
    let mut volume_key = [0u8; 32];
    OsRng.fill_bytes(&mut volume_key);
    let wrapped = crate::passkey::encrypt_with_vault(&dek, &volume_key)?;
    volume_key.zeroize();
    fs::create_dir_all(dir.join("ciphertext"))?;
    let mount_path = volume_mount_path(config_dir, &name);
    if !cfg!(target_os = "macos") {
        fs::create_dir_all(&mount_path)?;
    }
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
        mount_path: mount_path.display().to_string(),
        backend: "pending-encrypted-mount".into(),
        created_at: chrono::Utc::now().to_rfc3339(),
    };
    fs::write(
        dir.join("volume.json"),
        serde_json::to_string_pretty(&metadata)?,
    )?;
    Ok(metadata)
}

pub fn volume_status(config_dir: &Path, name: &str) -> Result<VolumeMetadata> {
    let name = volume_name(name)?;
    let path = volume_root(config_dir).join(&name).join("volume.json");
    let mut metadata: VolumeMetadata = serde_json::from_str(
        &fs::read_to_string(&path).with_context(|| format!("missing {}", path.display()))?,
    )?;
    if cfg!(target_os = "macos") {
        metadata.mount_path = volume_mount_path(config_dir, &name).display().to_string();
    }
    Ok(metadata)
}

pub fn scaffold_web_service(path: &Path, name: &str, repository: &str) -> Result<()> {
    if path.exists() {
        bail!("{} already exists; refusing to overwrite", path.display());
    }
    if !valid_git_url(repository) {
        bail!("repository must be a whitespace-free HTTPS or SSH Git URL");
    }
    let manifest = WebServiceManifest {
        api_version: "grid/v1alpha1".into(),
        kind: "GridWebService".into(),
        metadata: EngineMetadata {
            name: volume_name(name)?,
        },
        spec: WebServiceSpec {
            // The private registry promotion step resolves this to an immutable digest.
            image: "caddy:2-alpine".into(),
            git: GitSource {
                repository: repository.into(),
                branch: default_branch(),
            },
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
    if m.api_version != "grid/v1alpha1" || m.kind != "GridWebService" {
        bail!("expected apiVersion grid/v1alpha1 and kind GridWebService");
    }
    if m.spec.image != "caddy:2-alpine" {
        bail!("phase 1 web services require exactly caddy:2-alpine");
    }
    if m.spec.exposure != "grid-tunnel" {
        bail!("web services must use exposure: grid-tunnel; direct host ports are forbidden");
    }
    if m.spec.service_port != default_service_port() {
        bail!("web services must use the GRID service port");
    }
    if !valid_git_url(&m.spec.git.repository) {
        bail!("git.repository must be a whitespace-free HTTPS or SSH URL");
    }
    if m.spec.volume != m.metadata.name {
        bail!("phase 1 requires one dedicated encrypted volume named exactly like the service");
    }
    if m.spec.git.branch.is_empty()
        || m.spec.git.branch.starts_with('-')
        || !m
            .spec
            .git
            .branch
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '/' | '_' | '-'))
    {
        bail!("git.branch contains unsupported characters");
    }
    Ok(m)
}

fn valid_git_url(value: &str) -> bool {
    !value.contains(char::is_whitespace)
        && (value.starts_with("https://")
            || value.starts_with("ssh://")
            || value.starts_with("git@"))
}

pub async fn create_service_deploy_key(
    config_dir: &Path,
    service: &str,
    repository: &str,
) -> Result<ServiceDeployKey> {
    let service = volume_name(service)?;
    if !valid_git_url(repository) {
        bail!("repository must be a whitespace-free HTTPS or SSH Git URL");
    }
    require(
        "ssh-keygen",
        "Install OpenSSH (ssh-keygen) before creating a deploy key.",
    )?;
    let root = config_dir.join("engine").join("services").join(&service);
    let private_path = root.join("deploy.key.enc");
    if private_path.exists() {
        bail!("a deploy key already exists for {service}; refusing to overwrite");
    }
    fs::create_dir_all(&root)?;
    let tmp = root.join(format!(".keygen-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&tmp)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&tmp, fs::Permissions::from_mode(0o700))?;
    }
    let generated = tmp.join("id_ed25519");
    let status = Command::new("ssh-keygen")
        .args([
            "-q",
            "-t",
            "ed25519",
            "-N",
            "",
            "-C",
            &format!("grid-engine:{service}"),
            "-f",
        ])
        .arg(&generated)
        .status()
        .context("generate SSH deploy key")?;
    if !status.success() {
        let _ = fs::remove_dir_all(&tmp);
        bail!("ssh-keygen failed");
    }
    let mut private = fs::read(&generated)?;
    let public = fs::read_to_string(generated.with_extension("pub"))?
        .trim()
        .to_string();
    let dek = crate::passkey::require_unlocked(config_dir, "encrypt Engine deploy key").await?;
    let sealed = crate::passkey::encrypt_with_vault(&dek, &private)?;
    private.zeroize();
    fs::write(&private_path, sealed)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&private_path, fs::Permissions::from_mode(0o600))?;
    }
    fs::remove_dir_all(&tmp)?;
    let key = ServiceDeployKey {
        service,
        repository: repository.into(),
        public_key: public,
        private_key_path: private_path.display().to_string(),
        created_at: chrono::Utc::now().to_rfc3339(),
    };
    fs::write(
        root.join("deploy-key.json"),
        serde_json::to_string_pretty(&key)?,
    )?;
    Ok(key)
}

fn service_root(config_dir: &Path, service: &str) -> Result<PathBuf> {
    Ok(config_dir
        .join("engine")
        .join("services")
        .join(volume_name(service)?))
}

fn runtime_path(config_dir: &Path, service: &str) -> Result<PathBuf> {
    Ok(service_root(config_dir, service)?.join("runtime.json"))
}

fn capability_path(config_dir: &Path, service: &str) -> Result<PathBuf> {
    Ok(service_root(config_dir, service)?.join("tunnel-capability.enc"))
}

fn run_with_secret_stdin(
    program: &str,
    args: &[&str],
    secret: &[u8],
    guest_sudo: bool,
) -> Result<()> {
    let mut command = if cfg!(target_os = "macos") {
        let mut command = Command::new("limactl");
        command.args(["shell", "grid-containerd"]);
        if guest_sudo {
            command.arg("sudo");
        }
        command.arg(program);
        command
    } else {
        Command::new(program)
    };
    let mut child = command
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .spawn()
        .with_context(|| format!("start {program}"))?;
    let mut stdin = child.stdin.take().context("open secret stdin")?;
    stdin.write_all(secret)?;
    stdin.write_all(b"\n")?;
    drop(stdin);
    let status = child.wait()?;
    if !status.success() {
        bail!("{program} failed with {status}");
    }
    Ok(())
}

fn macos_guest_owner() -> Result<String> {
    let read_id = |flag: &str| -> Result<String> {
        let output = Command::new("limactl")
            .args(["shell", "grid-containerd", "id", flag])
            .output()
            .context("read rootless Engine VM identity")?;
        if !output.status.success() {
            bail!("cannot read rootless Engine VM identity");
        }
        let value = String::from_utf8(output.stdout)?.trim().to_string();
        if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
            bail!("invalid rootless Engine VM identity");
        }
        Ok(value)
    };
    Ok(format!("{}:{}", read_id("-u")?, read_id("-g")?))
}

async fn mount_volume(volume: &VolumeMetadata, dek: &[u8; 32]) -> Result<()> {
    if !engine_tool_exists("gocryptfs") {
        bail!("Install gocryptfs before deploying an Engine service.");
    }
    let encrypted = fs::read(&volume.key_path).context("read wrapped volume key")?;
    let mut key = crate::passkey::decrypt_with_vault(dek, &encrypted)?;
    let mut passphrase = hex::encode(&key).into_bytes();
    key.zeroize();
    let cipher = Path::new(&volume.encrypted_path);
    let mount = Path::new(&volume.mount_path);
    fs::create_dir_all(cipher)?;
    if cfg!(target_os = "macos") {
        let status = Command::new("limactl")
            .args(["shell", "grid-containerd", "mkdir", "-p"])
            .arg(mount)
            .status()
            .context("create VM-native encrypted volume mountpoint")?;
        if !status.success() {
            bail!("cannot create VM-native encrypted volume mountpoint");
        }
    } else {
        fs::create_dir_all(mount)?;
    }
    if !cipher.join("gocryptfs.conf").exists() {
        run_with_secret_stdin(
            "gocryptfs",
            &["-q", "-init", "--", &volume.encrypted_path],
            &passphrase,
            false,
        )?;
    }
    let guest_owner = if cfg!(target_os = "macos") {
        Some(macos_guest_owner()?)
    } else {
        None
    };
    let mount_args = if let Some(owner) = guest_owner.as_deref() {
        vec![
            "-q",
            "-allow_other",
            "-force_owner",
            owner,
            "--",
            &volume.encrypted_path,
            &volume.mount_path,
        ]
    } else {
        vec!["-q", "--", &volume.encrypted_path, &volume.mount_path]
    };
    let mount_result = run_with_secret_stdin(
        "gocryptfs",
        &mount_args,
        &passphrase,
        cfg!(target_os = "macos"),
    );
    passphrase.zeroize();
    mount_result?;
    Ok(())
}

fn unmount_volume(volume: &VolumeMetadata) -> Result<()> {
    let mount = &volume.mount_path;
    let status = if cfg!(target_os = "macos") {
        Command::new("limactl")
            .args(["shell", "grid-containerd", "sudo", "umount", mount])
            .status()
    } else if command_exists("fusermount3") {
        Command::new("fusermount3").args(["-u", mount]).status()
    } else {
        Command::new("fusermount").args(["-u", mount]).status()
    }
    .context("unmount encrypted Engine volume")?;
    if !status.success() {
        bail!("encrypted volume unmount failed: {status}");
    }
    Ok(())
}

fn with_ephemeral_deploy_key<T>(
    config_dir: &Path,
    service: &str,
    dek: &[u8; 32],
    f: impl FnOnce(Option<&Path>) -> Result<T>,
) -> Result<T> {
    let root = service_root(config_dir, service)?;
    let wrapped_path = root.join("deploy.key.enc");
    if !wrapped_path.exists() {
        return f(None);
    }
    let wrapped = fs::read(&wrapped_path)?;
    let mut private = crate::passkey::decrypt_with_vault(dek, &wrapped)?;
    let tmp = root.join(format!(".deploy-{}", uuid::Uuid::new_v4()));
    fs::write(&tmp, &private)?;
    private.zeroize();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&tmp, fs::Permissions::from_mode(0o600))?;
    }
    let result = f(Some(&tmp));
    let _ = fs::remove_file(&tmp);
    result
}

fn sync_repository(
    config_dir: &Path,
    manifest: &WebServiceManifest,
    source: &Path,
    dek: &[u8; 32],
) -> Result<String> {
    if !engine_tool_exists("git") {
        bail!("Install Git before deploying an Engine service.");
    }
    let ssh_repo = manifest.spec.git.repository.starts_with("git@")
        || manifest.spec.git.repository.starts_with("ssh://");
    if ssh_repo
        && !service_root(config_dir, &manifest.metadata.name)?
            .join("deploy.key.enc")
            .exists()
    {
        bail!(
            "private SSH repository requires a per-service deploy key; run `grid engine service key-create`"
        );
    }
    with_ephemeral_deploy_key(config_dir, &manifest.metadata.name, dek, |deploy_key| {
        let ssh_command = deploy_key.map(|path| {
            format!(
                "ssh -i {} -o IdentitiesOnly=yes -o StrictHostKeyChecking=accept-new",
                shell_quote(&path.display().to_string())
            )
        });
        if !source.join(".git").exists() {
            let mut cmd = engine_git_command(ssh_command.as_deref());
            let status = cmd
                .args([
                    "clone",
                    "--depth",
                    "1",
                    "--branch",
                    &manifest.spec.git.branch,
                ])
                .arg(&manifest.spec.git.repository)
                .arg(source)
                .status()
                .context("clone service repository")?;
            if !status.success() {
                bail!("Git clone failed");
            }
        } else {
            let mut cmd = engine_git_command(ssh_command.as_deref());
            let status = cmd
                .arg("-C")
                .arg(source)
                .args(["pull", "--ff-only", "origin", &manifest.spec.git.branch])
                .status()
                .context("update service repository")?;
            if !status.success() {
                bail!("Git fast-forward update failed");
            }
        }
        let output = engine_git_command(None)
            .arg("-C")
            .arg(source)
            .args(["rev-parse", "HEAD"])
            .output()?;
        if !output.status.success() {
            bail!("cannot determine deployed Git commit");
        }
        Ok(String::from_utf8(output.stdout)?.trim().to_string())
    })
}

fn engine_git_command(ssh_command: Option<&str>) -> Command {
    if cfg!(target_os = "macos") {
        let mut command = Command::new("limactl");
        command.args(["shell", "grid-containerd"]);
        if let Some(ssh) = ssh_command {
            command.arg("env");
            command.arg(format!("GIT_SSH_COMMAND={ssh}"));
        }
        command.arg("git");
        command
    } else {
        let mut command = Command::new("git");
        if let Some(ssh) = ssh_command {
            command.env("GIT_SSH_COMMAND", ssh);
        }
        command
    }
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

pub async fn deploy_web_service(config_dir: &Path, manifest_path: &Path) -> Result<ServiceRuntime> {
    let manifest = load_web_service(manifest_path)?;
    if !crate::compute::containerd_available().await {
        bail!("containerd/nerdctl unavailable; run `grid engine install`");
    }
    let service = manifest.metadata.name.clone();
    fs::create_dir_all(service_root(config_dir, &service)?)?;
    let existing_receipt = runtime_path(config_dir, &service)?;
    if existing_receipt.exists() && service_status(config_dir, &service)?.state == "running-private"
    {
        bail!("service {service} is already running; stop it before redeploying");
    }
    let volume = match volume_status(config_dir, &manifest.spec.volume) {
        Ok(volume) => volume,
        Err(_) => prepare_volume(config_dir, &manifest.spec.volume).await?,
    };
    let dek = crate::passkey::require_unlocked(config_dir, "deploy private Engine service").await?;
    mount_volume(&volume, &dek).await?;
    let source = Path::new(&volume.mount_path).join("source");
    let commit = match sync_repository(config_dir, &manifest, &source, &dek) {
        Ok(commit) => commit,
        Err(error) => {
            let _ = unmount_volume(&volume);
            return Err(error);
        }
    };
    let (container, image_digest) = match crate::compute::run_caddy_service(
        &service,
        &manifest.spec.image,
        &source,
        manifest.spec.service_port,
    )
    .await
    {
        Ok(container) => container,
        Err(error) => {
            let _ = unmount_volume(&volume);
            return Err(error);
        }
    };
    let finalized = (|| -> Result<ServiceRuntime> {
        let mut capability = [0u8; 32];
        OsRng.fill_bytes(&mut capability);
        let sealed_capability = crate::passkey::encrypt_with_vault(&dek, &capability)?;
        capability.zeroize();
        let cap_path = capability_path(config_dir, &service)?;
        fs::write(&cap_path, sealed_capability)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&cap_path, fs::Permissions::from_mode(0o600))?;
        }
        let mut runtime = ServiceRuntime {
            service: service.clone(),
            state: "running-private".into(),
            container: container.clone(),
            locator: format!("grid://service/{service}"),
            repository: manifest.spec.git.repository,
            commit,
            image: manifest.spec.image,
            image_digest,
            volume: manifest.spec.volume,
            loopback_port: manifest.spec.service_port,
            public_exposure: false,
            started_at: chrono::Utc::now().to_rfc3339(),
            operator_pubkey: crate::passkey::operator_pubkey_hex(config_dir)?,
            receipt_signature: String::new(),
        };
        let unsigned = serde_json::to_vec(&runtime)?;
        runtime.receipt_signature = crate::passkey::sign_operator(config_dir, &dek, &unsigned)?;
        fs::write(
            runtime_path(config_dir, &service)?,
            serde_json::to_vec_pretty(&runtime)?,
        )?;
        Ok(runtime)
    })();
    if finalized.is_err() {
        let _ = crate::compute::rm_container(&container);
        let _ = unmount_volume(&volume);
    }
    finalized
}

pub fn service_status(config_dir: &Path, service: &str) -> Result<ServiceRuntime> {
    let path = runtime_path(config_dir, service)?;
    let runtime: ServiceRuntime = serde_json::from_slice(
        &fs::read(&path).with_context(|| format!("missing {}", path.display()))?,
    )
    .context("invalid Engine runtime receipt")?;
    let mut unsigned = runtime.clone();
    let signature = std::mem::take(&mut unsigned.receipt_signature);
    crate::passkey::verify_operator_sig(
        &runtime.operator_pubkey,
        &serde_json::to_vec(&unsigned)?,
        &signature,
    )
    .context("invalid Engine runtime receipt signature")?;
    Ok(runtime)
}

pub fn service_logs(config_dir: &Path, service: &str, follow: bool) -> Result<()> {
    let runtime = service_status(config_dir, service)?;
    crate::compute::container_logs(&runtime.container, follow)
}

pub async fn stop_web_service(config_dir: &Path, service: &str) -> Result<ServiceRuntime> {
    let mut runtime = service_status(config_dir, service)?;
    if runtime.state == "stopped" {
        return Ok(runtime);
    }
    crate::compute::stop_container(&runtime.container)?;
    crate::compute::rm_container(&runtime.container)?;
    let volume = volume_status(config_dir, &runtime.volume)?;
    unmount_volume(&volume)?;
    runtime.state = "stopped".into();
    let dek = crate::passkey::require_unlocked(config_dir, "sign Engine stop receipt").await?;
    runtime.receipt_signature.clear();
    runtime.receipt_signature =
        crate::passkey::sign_operator(config_dir, &dek, &serde_json::to_vec(&runtime)?)?;
    fs::write(
        runtime_path(config_dir, service)?,
        serde_json::to_vec_pretty(&runtime)?,
    )?;
    Ok(runtime)
}

pub async fn destroy_web_service(config_dir: &Path, service: &str) -> Result<()> {
    let runtime = service_status(config_dir, service)?;
    if runtime.state == "running-private" {
        stop_web_service(config_dir, service).await?;
    }
    let service_dir = service_root(config_dir, service)?;
    let volume_dir = volume_root(config_dir).join(volume_name(&runtime.volume)?);
    if service_dir.exists() {
        fs::remove_dir_all(&service_dir)
            .with_context(|| format!("remove {}", service_dir.display()))?;
    }
    if volume_dir.exists() {
        fs::remove_dir_all(&volume_dir)
            .with_context(|| format!("remove {}", volume_dir.display()))?;
    }
    Ok(())
}

/// Verify and consume a one-time tunnel capability without unlocking the
/// operator vault. Only a SHA-256 verifier is present in the P2P process.
pub fn tunnel_authorization_message(service: &str, capability: &str) -> Vec<u8> {
    format!("GRID-ENGINE-TUNNEL-V1\n{service}\n{capability}").into_bytes()
}

pub fn consume_private_capability(
    config_dir: &Path,
    service: &str,
    candidate: &str,
    client_pubkey: &str,
    client_signature: &str,
) -> bool {
    let Ok(raw) = hex::decode(candidate) else {
        return false;
    };
    if raw.len() != 32 {
        return false;
    }
    let Ok(root) = service_root(config_dir, service) else {
        return false;
    };
    let path = root.join("tunnel-capability.json");
    let Ok(encoded) = fs::read(&path) else {
        return false;
    };
    let Ok(expected) = serde_json::from_slice::<TunnelCapabilityVerifier>(&encoded) else {
        return false;
    };
    if expected.service != service || expected.client_pubkey != client_pubkey {
        return false;
    }
    let actual = hex::encode(sha2::Sha256::digest(&raw));
    if actual != expected.capability_sha256 {
        return false;
    }
    if crate::passkey::verify_operator_sig(
        client_pubkey,
        &tunnel_authorization_message(service, candidate),
        client_signature,
    )
    .is_err()
    {
        return false;
    }
    fs::remove_file(path).is_ok()
}

pub async fn reveal_private_capability(
    config_dir: &Path,
    service: &str,
    client_pubkey: &str,
) -> Result<String> {
    let client_key = hex::decode(client_pubkey).context("decode assigned client public key")?;
    if client_key.len() != 32 {
        bail!("assigned client public key must be 32-byte hex");
    }
    let dek =
        crate::passkey::require_unlocked(config_dir, "reveal private service capability").await?;
    let mut raw = crate::passkey::decrypt_with_vault(
        &dek,
        &fs::read(capability_path(config_dir, service)?)?,
    )?;
    let encoded = hex::encode(&raw);
    let verifier = TunnelCapabilityVerifier {
        service: volume_name(service)?,
        capability_sha256: hex::encode(sha2::Sha256::digest(&raw)),
        client_pubkey: client_pubkey.to_ascii_lowercase(),
        issued_at: chrono::Utc::now().to_rfc3339(),
    };
    let verifier_path = service_root(config_dir, service)?.join("tunnel-capability.json");
    fs::write(&verifier_path, serde_json::to_vec_pretty(&verifier)?)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&verifier_path, fs::Permissions::from_mode(0o600))?;
    }
    raw.zeroize();
    Ok(encoded)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn private_capability_is_one_time_and_service_scoped() {
        use ed25519_dalek::{Signer, SigningKey};
        let dir = tempfile::tempdir().unwrap();
        let service = "site-a";
        let root = service_root(dir.path(), service).unwrap();
        fs::create_dir_all(&root).unwrap();
        let capability = [7u8; 32];
        let signing = SigningKey::from_bytes(&[9u8; 32]);
        let client_pubkey = hex::encode(signing.verifying_key().to_bytes());
        let verifier = TunnelCapabilityVerifier {
            service: service.into(),
            capability_sha256: hex::encode(sha2::Sha256::digest(capability)),
            client_pubkey: client_pubkey.clone(),
            issued_at: "now".into(),
        };
        fs::write(
            root.join("tunnel-capability.json"),
            serde_json::to_vec(&verifier).unwrap(),
        )
        .unwrap();
        let token = hex::encode(capability);
        let signature = hex::encode(
            signing
                .sign(&tunnel_authorization_message(service, &token))
                .to_bytes(),
        );
        assert!(!consume_private_capability(
            dir.path(),
            "site-b",
            &token,
            &client_pubkey,
            &signature
        ));
        assert!(!consume_private_capability(
            dir.path(),
            service,
            "not-hex",
            &client_pubkey,
            &signature
        ));
        assert!(!consume_private_capability(
            dir.path(),
            service,
            &token,
            &"22".repeat(32),
            &signature
        ));
        assert!(consume_private_capability(
            dir.path(),
            service,
            &token,
            &client_pubkey,
            &signature
        ));
        assert!(!consume_private_capability(
            dir.path(),
            service,
            &token,
            &client_pubkey,
            &signature
        ));
    }

    #[test]
    fn web_service_manifest_rejects_public_exposure() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("site.yaml");
        scaffold_web_service(&path, "site", "https://example.com/site.git").unwrap();
        let mut manifest: WebServiceManifest =
            serde_yaml::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        manifest.spec.exposure = "public".into();
        fs::write(&path, serde_yaml::to_string(&manifest).unwrap()).unwrap();
        assert!(load_web_service(&path).is_err());
    }
}
