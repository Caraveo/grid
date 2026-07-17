//! Public mesh registry client — **https://grid-compute.com**
//!
//! * `GET  {registry}/api/registry` — list peers (location-only public fields)
//! * `POST {registry}/api/mesh/ping` — location-only globe pulse
//!
//! Never sends IPs, ports, hostnames, wallets, or coordinator URLs.
//! Globe *write* is opt-in (coords required). Registry *read* always uses the site.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use tracing::{debug, info, warn};

use crate::config::NodeConfig;

/// Canonical public mesh registry (Cloudflare).
pub const DEFAULT_REGISTRY_URL: &str = "https://grid-compute.com";

/// Minimum seconds between globe pings (debounce).
const DEBOUNCE_SECS: u64 = 5 * 60;

static LAST_PING_UNIX: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GlobePingBody {
    pub node_id: String,
    pub label: String,
    pub class: String,
    pub region: String,
    pub status: String,
    pub lat: f64,
    pub lng: f64,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RegistryNode {
    pub id: String,
    pub label: String,
    pub class: String,
    pub region: String,
    pub status: String,
    #[serde(default)]
    pub role: Option<String>,
    #[serde(default)]
    pub joined_at: Option<String>,
    #[serde(default)]
    pub last_seen: Option<String>,
    #[serde(default)]
    pub lat: Option<f64>,
    #[serde(default)]
    pub lng: Option<f64>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RegistrySnapshot {
    #[serde(default)]
    pub registry: Option<String>,
    pub phase: String,
    pub updated_at: String,
    pub genesis: RegistryNode,
    #[serde(default)]
    pub peers: Vec<RegistryNode>,
    #[serde(default)]
    pub nodes: Vec<RegistryNode>,
    #[serde(default)]
    pub stats: Option<serde_json::Value>,
    /// Compute capacity from site registry (may be empty on older servers).
    #[serde(default)]
    pub computes: Vec<serde_json::Value>,
    #[serde(default)]
    pub compute_stats: Option<serde_json::Value>,
}

fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Normalize a site/registry base URL (always https for bare hostnames).
pub fn normalize_base(raw: &str) -> String {
    let s = raw.trim().trim_end_matches('/').to_string();
    if s.starts_with("http://") || s.starts_with("https://") {
        s
    } else {
        format!("https://{s}")
    }
}

/// Public registry base URL.
///
/// Order: `GRID_REGISTRY_URL` → `GRID_SITE_URL` → **https://grid-compute.com**
pub fn registry_url() -> String {
    std::env::var("GRID_REGISTRY_URL")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .or_else(|| {
            std::env::var("GRID_SITE_URL")
                .ok()
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
        })
        .map(|s| normalize_base(&s))
        .unwrap_or_else(|| DEFAULT_REGISTRY_URL.to_string())
}

/// Alias used by globe ping (same host as the registry).
fn site_url() -> String {
    registry_url()
}

fn webhook_secret() -> Option<String> {
    std::env::var("GRID_WEBHOOK_SECRET")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// Resolve opt-in globe coords: env wins, then config.
pub fn resolve_coords(cfg: &NodeConfig) -> Option<(f64, f64, String)> {
    let lat = std::env::var("GRID_GLOBE_LAT")
        .ok()
        .and_then(|s| s.parse().ok())
        .or(cfg.globe_lat);
    let lng = std::env::var("GRID_GLOBE_LNG")
        .ok()
        .and_then(|s| s.parse().ok())
        .or(cfg.globe_lng);
    let region = std::env::var("GRID_GLOBE_REGION")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .or_else(|| cfg.globe_region.clone())
        .unwrap_or_else(|| {
            if cfg.region.is_empty() || cfg.region == "local" {
                "NA-W".into()
            } else {
                cfg.region.clone()
            }
        });

    match (lat, lng) {
        (Some(la), Some(lo)) if la.is_finite() && lo.is_finite() => {
            if !(-90.0..=90.0).contains(&la) || !(-180.0..=180.0).contains(&lo) {
                return None;
            }
            Some((la, lo, region))
        }
        _ => None,
    }
}

/// Fetch the public mesh registry from grid-compute.com (or override URL).
pub async fn fetch_registry(base: Option<&str>) -> Result<RegistrySnapshot> {
    let base = base
        .map(normalize_base)
        .unwrap_or_else(registry_url);
    let url = format!("{base}/api/registry");
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
        anyhow::bail!("registry HTTP {} from {url}", res.status());
    }
    let mut snap: RegistrySnapshot = res.json().await.context("parse registry JSON")?;
    if snap.registry.is_none() {
        snap.registry = Some(base);
    }
    Ok(snap)
}

/// Pretty-print registry for CLI.
pub fn print_registry(snap: &RegistrySnapshot, json: bool) -> Result<()> {
    if json {
        println!("{}", serde_json::to_string_pretty(snap)?);
        return Ok(());
    }
    let base = snap
        .registry
        .as_deref()
        .unwrap_or(DEFAULT_REGISTRY_URL);
    println!("GRID public mesh registry");
    println!("  url       {base}");
    println!("  phase     {}", snap.phase);
    println!("  updated   {}", snap.updated_at);
    if let Some(ref s) = snap.stats {
        if let (Some(t), Some(o), Some(p)) = (
            s.get("total").and_then(|v| v.as_u64()),
            s.get("online").and_then(|v| v.as_u64()),
            s.get("peers").and_then(|v| v.as_u64()),
        ) {
            println!("  stats     total={t} online={o} peers={p}");
        }
    }
    println!();
    println!(
        "  {:12} {:16} {:5} {:10} {:8} {}",
        "ID", "LABEL", "CLASS", "REGION", "STATUS", "ROLE"
    );
    println!("  {}", "-".repeat(70));
    let g = &snap.genesis;
    println!(
        "  {:12} {:16} {:5} {:10} {:8} {}",
        short_id(&g.id),
        trunc(&g.label, 16),
        g.class,
        trunc(&g.region, 10),
        g.status,
        g.role.as_deref().unwrap_or("genesis")
    );
    for p in &snap.peers {
        println!(
            "  {:12} {:16} {:5} {:10} {:8} {}",
            short_id(&p.id),
            trunc(&p.label, 16),
            p.class,
            trunc(&p.region, 10),
            p.status,
            p.role.as_deref().unwrap_or("peer")
        );
    }
    if snap.peers.is_empty() {
        println!();
        println!("  (no peers yet — run `grid node` with globe coords to join)");
        println!("  Globe: https://grid-compute.com/#nodes");
    }

    // Compute capacity (from same registry response when present)
    if let Some(ref cs) = snap.compute_stats {
        println!();
        println!("Computes (capacity registry)");
        println!(
            "  available={} busy={} offline={} freeSlots={}",
            cs.get("available").and_then(|v| v.as_u64()).unwrap_or(0),
            cs.get("busy").and_then(|v| v.as_u64()).unwrap_or(0),
            cs.get("offline").and_then(|v| v.as_u64()).unwrap_or(0),
            cs.get("freeSlots").and_then(|v| v.as_u64()).unwrap_or(0),
        );
        println!("  detail    grid compute available");
        println!("  api       {base}/api/registry/computes?available=1");
    } else if !snap.computes.is_empty() {
        println!();
        println!("Computes: {} listed — grid compute available", snap.computes.len());
    }
    Ok(())
}

fn short_id(id: &str) -> String {
    if id == "genesis" {
        return "GENESIS".into();
    }
    let clean = id.trim_start_matches("node_").trim_start_matches("node-");
    clean.chars().take(10).collect()
}

fn trunc(s: &str, n: usize) -> String {
    let t: String = s.chars().take(n).collect();
    t
}

/// Fire-and-forget friendly: never panics; does not block mining.
/// `force` = true on node start (still respects missing coords).
pub async fn ping_globe(cfg: &NodeConfig, force: bool) {
    let base = site_url();

    let Some((lat, lng, region)) = resolve_coords(cfg) else {
        info!("globe ping skipped (no coords) — set GRID_GLOBE_LAT/LNG to join registry");
        return;
    };

    if !force {
        let now = now_unix();
        let last = LAST_PING_UNIX.load(Ordering::Relaxed);
        if now.saturating_sub(last) < DEBOUNCE_SECS {
            debug!("globe ping debounced");
            return;
        }
    }

    // Sanitize label: keep simple chars for site allowlist
    let label: String = cfg
        .name
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, ' ' | '_' | '-' | '.' | '\''))
        .take(32)
        .collect::<String>()
        .trim()
        .to_string();
    let label = if label.is_empty() {
        "node".into()
    } else {
        label
    };

    let body = GlobePingBody {
        node_id: cfg.node_id.clone(),
        label,
        class: cfg.class.as_str().to_string(),
        region,
        status: "online".into(),
        lat,
        lng,
    };

    let url = format!("{base}/api/mesh/ping");
    let client = match reqwest::Client::builder()
        .timeout(Duration::from_secs(3))
        .build()
    {
        Ok(c) => c,
        Err(e) => {
            warn!("globe ping failed (client): {e}");
            return;
        }
    };

    let mut req = client.post(&url).json(&body);
    if let Some(secret) = webhook_secret() {
        req = req.header("Authorization", format!("Bearer {secret}"));
    }

    match req.send().await {
        Ok(res) if res.status().is_success() => {
            LAST_PING_UNIX.store(now_unix(), Ordering::Relaxed);
            info!("registry ping ok → {url}");
        }
        Ok(res) => {
            let status = res.status();
            let text = res.text().await.unwrap_or_default();
            warn!("registry ping failed HTTP {status}: {text}");
        }
        Err(e) => {
            warn!("registry ping failed: {e}");
        }
    }
}
