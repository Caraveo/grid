//! Portable compute manifests — survive machine changes.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

pub const DEFAULT_IMAGE: &str = "alpine:3.20";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ComputeVisibility {
    Public,
    Private,
}

impl ComputeVisibility {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Public => "public",
            Self::Private => "private",
        }
    }

    pub fn parse(s: &str) -> Result<Self> {
        match s.trim().to_lowercase().as_str() {
            "public" | "pub" => Ok(Self::Public),
            "private" | "priv" => Ok(Self::Private),
            o => anyhow::bail!("visibility must be public|private, got {o}"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ComputeManifest {
    pub name: String,
    pub image: String,
    pub visibility: ComputeVisibility,
    pub backend: String,
    pub cpus: f64,
    pub memory_mb: u64,
    pub replicas: u32,
    pub class: String,
    #[serde(default)]
    pub port: Option<u16>,
    pub created_at: String,
    pub machine_id: String,
    #[serde(default)]
    pub public_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ComputeStatus {
    pub name: String,
    pub machine_id: String,
    #[serde(default)]
    pub container_ids: Vec<String>,
    pub state: String,
    #[serde(default)]
    pub public_url: Option<String>,
    #[serde(default)]
    pub last_error: Option<String>,
    pub updated_at: String,
}

pub fn computes_root(config_dir: &Path) -> PathBuf {
    config_dir.join("computes")
}

pub fn compute_dir(config_dir: &Path, name: &str) -> PathBuf {
    computes_root(config_dir).join(name)
}

pub fn machine_id(config_dir: &Path) -> Result<String> {
    let root = computes_root(config_dir);
    fs::create_dir_all(&root)?;
    let path = root.join("machine-id");
    if path.exists() {
        return Ok(fs::read_to_string(&path)?.trim().to_string());
    }
    let id = format!("mach_{}", &uuid::Uuid::new_v4().to_string()[..12]);
    fs::write(&path, &id)?;
    Ok(id)
}

pub fn save_manifest(config_dir: &Path, m: &ComputeManifest) -> Result<()> {
    let dir = compute_dir(config_dir, &m.name);
    fs::create_dir_all(&dir)?;
    let path = dir.join("manifest.json");
    fs::write(&path, serde_json::to_string_pretty(m)?)?;
    Ok(())
}

pub fn load_manifest(config_dir: &Path, name: &str) -> Result<ComputeManifest> {
    let path = compute_dir(config_dir, name).join("manifest.json");
    let raw = fs::read_to_string(&path).with_context(|| format!("missing {}", path.display()))?;
    Ok(serde_json::from_str(&raw)?)
}

pub fn save_status(config_dir: &Path, s: &ComputeStatus) -> Result<()> {
    let dir = compute_dir(config_dir, &s.name);
    fs::create_dir_all(&dir)?;
    fs::write(dir.join("status.json"), serde_json::to_string_pretty(s)?)?;
    Ok(())
}

pub fn load_status(config_dir: &Path, name: &str) -> Result<ComputeStatus> {
    let path = compute_dir(config_dir, name).join("status.json");
    let raw = fs::read_to_string(&path)?;
    Ok(serde_json::from_str(&raw)?)
}

pub fn list_computes(config_dir: &Path) -> Result<Vec<ComputeManifest>> {
    let root = computes_root(config_dir);
    if !root.exists() {
        return Ok(vec![]);
    }
    let mut out = vec![];
    for ent in fs::read_dir(&root)? {
        let ent = ent?;
        if !ent.file_type()?.is_dir() {
            continue;
        }
        let name = ent.file_name().to_string_lossy().to_string();
        if let Ok(m) = load_manifest(config_dir, &name) {
            out.push(m);
        }
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(out)
}

pub fn remove_compute(config_dir: &Path, name: &str) -> Result<()> {
    let dir = compute_dir(config_dir, name);
    if dir.exists() {
        fs::remove_dir_all(&dir)?;
    }
    Ok(())
}

pub fn export_compute(config_dir: &Path, name: &str) -> Result<String> {
    let m = load_manifest(config_dir, name)?;
    Ok(serde_json::to_string_pretty(&m)?)
}

pub fn import_compute(config_dir: &Path, json: &str) -> Result<ComputeManifest> {
    let mut m: ComputeManifest = serde_json::from_str(json)?;
    m.machine_id = machine_id(config_dir)?;
    m.public_url = None; // rebind tunnel on new host
    save_manifest(config_dir, &m)?;
    Ok(m)
}
