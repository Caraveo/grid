//! Announce / query compute capacity on **https://grid-compute.com**.
//!
//! * `GET  /api/registry/computes` — list / availability  
//! * `POST /api/registry/computes` — host heartbeat (webhook secret in prod)

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::time::Duration;
use tracing::{debug, info, warn};

use super::manifest::{list_computes, load_status};
use crate::mesh_ping::{normalize_base, registry_url};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PublicCompute {
    pub id: String,
    pub name: String,
    pub node_id: String,
    #[serde(default)]
    pub label: String,
    pub image: String,
    pub visibility: String,
    pub class: String,
    #[serde(default)]
    pub backend: String,
    #[serde(default)]
    pub replicas: u32,
    #[serde(default)]
    pub free_slots: u32,
    pub status: String,
    #[serde(default)]
    pub last_seen: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ComputesResponse {
    #[serde(default)]
    pub registry: Option<String>,
    #[serde(default)]
    pub computes: Vec<PublicCompute>,
    #[serde(default)]
    pub stats: Option<serde_json::Value>,
    #[serde(default)]
    pub available_ms: Option<u64>,
}

fn webhook_secret() -> Option<String> {
    std::env::var("GRID_WEBHOOK_SECRET")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// Announce local computes to the public registry (fire-and-forget safe).
pub async fn announce_computes(config_dir: &Path, node_id: &str, label: &str) {
    let items = list_computes(config_dir).unwrap_or_default();
    if items.is_empty() {
        debug!("compute registry announce skipped (no local computes)");
        return;
    }

    let computes: Vec<serde_json::Value> = items
        .iter()
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
                .unwrap_or(0);
            let status = if free > 0 { "available" } else { "busy" };
            serde_json::json!({
                "name": m.name,
                "image": m.image,
                "visibility": m.visibility.as_str(),
                "class": m.class,
                "backend": m.backend,
                "replicas": m.replicas,
                "freeSlots": free,
                "status": status,
            })
        })
        .collect();

    let base = registry_url();
    let url = format!("{base}/api/registry/computes");
    let body = serde_json::json!({
        "nodeId": node_id,
        "label": label,
        "computes": computes,
    });

    let client = match reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
    {
        Ok(c) => c,
        Err(e) => {
            warn!("compute announce client: {e}");
            return;
        }
    };

    let mut req = client.post(&url).json(&body);
    if let Some(secret) = webhook_secret() {
        req = req.header("Authorization", format!("Bearer {secret}"));
    }

    match req.send().await {
        Ok(res) if res.status().is_success() => {
            info!(
                "compute registry announce ok → {url} ({} compute(s))",
                items.len()
            );
        }
        Ok(res) => {
            let status = res.status();
            let text = res.text().await.unwrap_or_default();
            warn!("compute registry announce HTTP {status}: {text}");
        }
        Err(e) => warn!("compute registry announce failed: {e}"),
    }
}

/// Fetch compute registry (optionally only available).
pub async fn fetch_computes(base: Option<&str>, available_only: bool) -> Result<ComputesResponse> {
    let base = base.map(normalize_base).unwrap_or_else(registry_url);
    let url = if available_only {
        format!("{base}/api/registry/computes?available=1")
    } else {
        format!("{base}/api/registry/computes")
    };
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(8))
        .build()
        .context("http client")?;
    let res = client
        .get(&url)
        .header("Accept", "application/json")
        .send()
        .await
        .with_context(|| format!("GET {url}"))?;
    if !res.status().is_success() {
        anyhow::bail!("computes registry HTTP {} from {url}", res.status());
    }
    let mut snap: ComputesResponse = res.json().await.context("parse computes JSON")?;
    if snap.registry.is_none() {
        snap.registry = Some(base);
    }
    Ok(snap)
}

pub fn print_computes(snap: &ComputesResponse, json: bool) -> Result<()> {
    if json {
        println!("{}", serde_json::to_string_pretty(snap)?);
        return Ok(());
    }
    let base = snap
        .registry
        .as_deref()
        .unwrap_or("https://grid-compute.com");
    println!("GRID compute registry");
    println!("  url       {base}/api/registry/computes");
    if let Some(ref s) = snap.stats {
        println!(
            "  stats     available={} busy={} offline={} freeSlots={}",
            s.get("available").and_then(|v| v.as_u64()).unwrap_or(0),
            s.get("busy").and_then(|v| v.as_u64()).unwrap_or(0),
            s.get("offline").and_then(|v| v.as_u64()).unwrap_or(0),
            s.get("freeSlots").and_then(|v| v.as_u64()).unwrap_or(0),
        );
    }
    println!();
    println!(
        "  {:10} {:12} {:10} {:8} {:6} {:8} {}",
        "STATUS", "NAME", "NODE", "VIS", "SLOTS", "CLASS", "IMAGE"
    );
    println!("  {}", "-".repeat(72));
    if snap.computes.is_empty() {
        println!("  (no computes announced — grid launch <name> && grid host)");
        return Ok(());
    }
    for c in &snap.computes {
        let node_short: String = c
            .node_id
            .trim_start_matches("node_")
            .chars()
            .take(10)
            .collect();
        println!(
            "  {:10} {:12} {:10} {:8} {:>2}/{:<3} {:8} {}",
            c.status,
            trunc(&c.name, 12),
            node_short,
            trunc(&c.visibility, 8),
            c.free_slots,
            c.replicas,
            trunc(&c.class, 8),
            trunc(&c.image, 24)
        );
    }
    Ok(())
}

fn trunc(s: &str, n: usize) -> String {
    s.chars().take(n).collect()
}
