//! Node configuration (`~/.grid/config.toml`).

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Capacity class — little miner through datacenter.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "UPPERCASE")]
pub enum NodeClass {
    #[default]
    S,
    M,
    L,
}

impl NodeClass {
    pub fn parse(s: &str) -> Result<Self> {
        match s.trim().to_uppercase().as_str() {
            "S" => Ok(Self::S),
            "M" => Ok(Self::M),
            "L" => Ok(Self::L),
            other => anyhow::bail!("class must be S|M|L, got {other}"),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::S => "S",
            Self::M => "M",
            Self::L => "L",
        }
    }
}

impl std::fmt::Display for NodeClass {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeConfig {
    pub name: String,
    pub node_id: String,
    #[serde(default)]
    pub class: NodeClass,
    #[serde(default = "default_region")]
    pub region: String,
    #[serde(default = "default_coordinator")]
    pub coordinator: String,
    #[serde(default = "default_gpu")]
    pub gpu_model: String,
    #[serde(default = "default_poll_ms")]
    pub poll_ms: u64,
    #[serde(default = "default_max_concurrent")]
    pub max_concurrent: u32,
    /// Operator identity cluster for whale emission caps (defaults to node_id).
    #[serde(default)]
    pub cluster_id: Option<String>,
    /// Opt-in public globe pin (WGS84). Omit to disable site mesh pings.
    #[serde(default)]
    pub globe_lat: Option<f64>,
    #[serde(default)]
    pub globe_lng: Option<f64>,
    /// Coarse region label for the globe (e.g. NA-W). Not a network endpoint.
    #[serde(default)]
    pub globe_region: Option<String>,
}

fn default_region() -> String {
    "local".into()
}
fn default_coordinator() -> String {
    "http://127.0.0.1:8787".into()
}
fn default_gpu() -> String {
    "cpu".into()
}
fn default_poll_ms() -> u64 {
    2000
}
fn default_max_concurrent() -> u32 {
    1
}

#[derive(Debug, Serialize, Deserialize)]
struct FileRoot {
    node: NodeConfig,
}

impl NodeConfig {
    pub fn default_dir() -> PathBuf {
        dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(".grid")
    }

    pub fn path_in(dir: &Path) -> PathBuf {
        dir.join("config.toml")
    }

    pub fn cluster(&self) -> &str {
        self.cluster_id.as_deref().unwrap_or(&self.node_id)
    }

    pub fn load(path: &Path) -> Result<Self> {
        let raw =
            std::fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
        let root: FileRoot = toml::from_str(&raw)?;
        Ok(root.node)
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let root = FileRoot { node: self.clone() };
        std::fs::write(path, toml::to_string_pretty(&root)?)?;
        Ok(())
    }

    pub fn init(
        dir: &Path,
        name: impl Into<String>,
        class: NodeClass,
        coordinator: impl Into<String>,
    ) -> Result<(Self, PathBuf)> {
        std::fs::create_dir_all(dir)?;
        let id = format!("node_{}", &uuid::Uuid::new_v4().to_string()[..8]);
        let cfg = Self {
            name: name.into(),
            node_id: id.clone(),
            class,
            region: default_region(),
            coordinator: coordinator.into(),
            gpu_model: default_gpu(),
            poll_ms: default_poll_ms(),
            max_concurrent: default_max_concurrent(),
            cluster_id: Some(id),
            globe_lat: None,
            globe_lng: None,
            globe_region: None,
        };
        let path = Self::path_in(dir);
        cfg.save(&path)?;
        Ok((cfg, path))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn roundtrip_config() {
        let dir = tempdir().unwrap();
        let (cfg, path) =
            NodeConfig::init(dir.path(), "test", NodeClass::S, "http://127.0.0.1:8787").unwrap();
        let loaded = NodeConfig::load(&path).unwrap();
        assert_eq!(loaded.name, "test");
        assert_eq!(loaded.node_id, cfg.node_id);
        assert_eq!(loaded.class, NodeClass::S);
    }
}
