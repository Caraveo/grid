//! Named computes — isolated containers you host on the grid.
//!
//! * **host** — pull useful `container_work`, serve in isolation, higher earn  
//! * **mine** — PoR / transactional-security work, slower earn  
//! * **launch** — name a compute capacity unit (`grid launch garage`)

mod allowlist;
mod docker;
mod isolation;
mod manifest;
mod registry;
mod serve;
mod tunnel;

pub use allowlist::{ensure_allowlist, is_image_allowed, DEFAULT_IMAGES};
pub use docker::{
    containerd_available, logs as container_logs, rm_container, run_caddy_service, stop_container,
    ContainerdError,
};
pub use manifest::{
    compute_dir, export_compute, import_compute, list_computes, load_manifest, load_status,
    machine_id, remove_compute, save_manifest, save_status, ComputeManifest, ComputeStatus,
    ComputeVisibility, DEFAULT_IMAGE,
};
pub use registry::{announce_computes, fetch_computes, print_computes};
pub use serve::{serve_container_job, ContainerJobSpec};
pub use tunnel::{public_endpoint_hint, GRID_CONTAINER_PORT};

use anyhow::{bail, Context, Result};
use std::path::Path;

/// Launch a named compute (capacity you manage exclusively).
pub async fn launch(
    config_dir: &Path,
    name: &str,
    image: &str,
    visibility: ComputeVisibility,
    backend: &str,
    cpus: f64,
    memory_mb: u64,
    replicas: u32,
    class: &str,
    port: Option<u16>,
) -> Result<ComputeManifest> {
    let name = sanitize_name(name)?;
    ensure_allowlist(config_dir)?;
    if !is_image_allowed(config_dir, image)? {
        bail!(
            "image '{image}' not on allowlist — edit {}/computes/allowlist.toml or pick: {}",
            config_dir.display(),
            DEFAULT_IMAGES.join(", ")
        );
    }

    let mid = machine_id(config_dir)?;
    let service_port = port.unwrap_or(GRID_CONTAINER_PORT);
    tunnel::validate_container_port(service_port)?;
    let mut manifest = ComputeManifest {
        name: name.clone(),
        image: image.to_string(),
        visibility,
        backend: backend.to_string(),
        cpus,
        memory_mb,
        replicas: replicas.max(1),
        class: class.to_string(),
        port: Some(service_port),
        created_at: chrono::Utc::now().to_rfc3339(),
        machine_id: mid.clone(),
        public_url: None,
    };

    match backend {
        "docker" => {
            if !containerd_available().await {
                bail!(
                    "containerd/nerdctl not available. Install nerdctl and start containerd; GRID refuses to fall back to Docker."
                );
            }
            docker::ensure_capacity(&manifest).await?;
            let mut status = ComputeStatus {
                name: name.clone(),
                machine_id: mid,
                container_ids: vec![],
                state: "ready".into(), // slots available for host pull-serve
                public_url: None,
                last_error: None,
                updated_at: chrono::Utc::now().to_rfc3339(),
            };
            if visibility == ComputeVisibility::Public {
                let hint = public_endpoint_hint(service_port);
                status.public_url = Some(hint.clone());
                manifest.public_url = Some(hint);
            }
            save_status(config_dir, &status)?;
        }
        "k8s" => {
            // Phase-1: record intent; operator applies manifests later / kind optional
            let mut status = ComputeStatus {
                name: name.clone(),
                machine_id: mid,
                container_ids: vec![],
                state: "registered".into(),
                public_url: None,
                last_error: Some(
                    "k8s backend: capacity registered; apply Deployment via kubectl (docker recommended for pilot)"
                        .into(),
                ),
                updated_at: chrono::Utc::now().to_rfc3339(),
            };
            if visibility == ComputeVisibility::Public {
                let hint = public_endpoint_hint(service_port);
                status.public_url = Some(hint.clone());
                manifest.public_url = Some(hint);
            }
            save_status(config_dir, &status)?;
        }
        other => bail!("unknown backend '{other}' (containerd|k8s)"),
    }

    save_manifest(config_dir, &manifest)?;

    // Best-effort: register capacity on public compute registry (grid-compute.com)
    let node_id = std::env::var("GRID_NODE_ID").unwrap_or_else(|_| {
        crate::config::NodeConfig::load(&crate::config::NodeConfig::path_in(config_dir))
            .map(|c| c.node_id)
            .unwrap_or_else(|_| format!("node_{name}"))
    });
    let label = crate::config::NodeConfig::load(&crate::config::NodeConfig::path_in(config_dir))
        .map(|c| c.name)
        .unwrap_or_else(|_| name.clone());
    announce_computes(config_dir, &node_id, &label).await;

    Ok(manifest)
}

pub fn stop(config_dir: &Path, name: &str) -> Result<()> {
    let name = sanitize_name(name)?;
    let manifest = load_manifest(config_dir, &name)?;
    if manifest.backend == "docker" {
        let st = load_status(config_dir, &name).ok();
        if let Some(st) = st {
            for id in &st.container_ids {
                let _ = docker::stop_container(id);
            }
        }
    }
    let mut st = load_status(config_dir, &name).unwrap_or(ComputeStatus {
        name: name.clone(),
        machine_id: machine_id(config_dir)?,
        container_ids: vec![],
        state: "stopped".into(),
        public_url: None,
        last_error: None,
        updated_at: chrono::Utc::now().to_rfc3339(),
    });
    st.state = "stopped".into();
    st.updated_at = chrono::Utc::now().to_rfc3339();
    save_status(config_dir, &st)?;
    Ok(())
}

pub fn destroy(config_dir: &Path, name: &str) -> Result<()> {
    let name = sanitize_name(name)?;
    let _ = stop(config_dir, &name);
    if let Ok(st) = load_status(config_dir, &name) {
        for id in &st.container_ids {
            let _ = docker::rm_container(id);
        }
    }
    remove_compute(config_dir, &name)?;
    Ok(())
}

pub fn print_list(config_dir: &Path) -> Result<()> {
    let items = list_computes(config_dir)?;
    if items.is_empty() {
        println!("No computes. Launch one:");
        println!("  grid launch garage --public");
        println!("  grid host          # pull & serve useful work");
        return Ok(());
    }
    println!(
        "{:16} {:10} {:8} {:6} {}",
        "NAME", "STATE", "VIS", "REPLICAS", "IMAGE"
    );
    for m in items {
        let st = load_status(config_dir, &m.name)
            .map(|s| s.state)
            .unwrap_or_else(|_| "unknown".into());
        println!(
            "{:16} {:10} {:8} {:6} {}",
            m.name,
            st,
            m.visibility.as_str(),
            m.replicas,
            m.image
        );
    }
    Ok(())
}

pub fn print_status(config_dir: &Path, name: &str) -> Result<()> {
    let name = sanitize_name(name)?;
    let m = load_manifest(config_dir, &name)?;
    let st = load_status(config_dir, &name).ok();
    println!("Compute:    {}", m.name);
    println!("Image:      {}", m.image);
    println!("Visibility: {}", m.visibility.as_str());
    println!("Backend:    {}", m.backend);
    println!("Class:      {}", m.class);
    println!("CPUs:       {}", m.cpus);
    println!("Memory:     {} MB", m.memory_mb);
    println!("Replicas:   {}", m.replicas);
    println!("Machine:    {}", m.machine_id);
    if let Some(u) = m
        .public_url
        .as_ref()
        .or(st.as_ref().and_then(|s| s.public_url.as_ref()))
    {
        println!("Public URL: {u}");
    }
    if let Some(st) = st {
        println!("State:      {}", st.state);
        if !st.container_ids.is_empty() {
            println!("Containers: {}", st.container_ids.join(", "));
        }
        if let Some(e) = st.last_error {
            println!("Error:      {e}");
        }
    }
    Ok(())
}

pub fn logs(config_dir: &Path, name: &str, follow: bool) -> Result<()> {
    let name = sanitize_name(name)?;
    let st = load_status(config_dir, &name).context("no status — launch first")?;
    let id = st
        .container_ids
        .first()
        .context("no containers — is Docker backend running?")?;
    docker::logs(id, follow)
}

pub fn sanitize_name(name: &str) -> Result<String> {
    let n = name.trim().to_lowercase();
    if n.is_empty() || n.len() > 48 {
        bail!("compute name must be 1–48 chars");
    }
    if !n
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        bail!("compute name: alphanumeric, - _ only");
    }
    if n == "genesis" {
        bail!("reserved name");
    }
    Ok(n)
}

/// Summaries for host heartbeat.
pub fn heartbeat_computes(config_dir: &Path) -> Vec<serde_json::Value> {
    list_computes(config_dir)
        .unwrap_or_default()
        .into_iter()
        .map(|m| {
            let st = load_status(config_dir, &m.name).ok();
            let free = st
                .as_ref()
                .map(|s| {
                    if s.state == "ready" || s.state == "running" || s.state == "registered" {
                        m.replicas
                    } else {
                        0
                    }
                })
                .unwrap_or(m.replicas);
            serde_json::json!({
                "name": m.name,
                "image": m.image,
                "visibility": m.visibility.as_str(),
                "replicas": m.replicas,
                "freeSlots": free,
                "backend": m.backend,
                "class": m.class,
            })
        })
        .collect()
}
