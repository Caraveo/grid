//! Public mesh registry client — **https://grid-compute.com**
//!
//! * `GET  {registry}/api/registry` — list peers (location-only public fields)
//! * `POST {registry}/api/mesh/ping` — location-only globe pulse
//!
//! Never sends IPs, ports, hostnames, wallets, or coordinator URLs.
//! Globe *write* is opt-in (coords required). Registry *read* always uses the site.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use ed25519_dalek::{Signer, SigningKey};
use rand::{rngs::OsRng, RngCore};
use serde::{Deserialize, Serialize};
use tracing::{debug, info, warn};

use crate::config::NodeConfig;

/// Canonical public mesh registry (Cloudflare).
pub const DEFAULT_REGISTRY_URL: &str = "https://grid-compute.com";

/// Minimum seconds between globe pings (debounce).
const DEBOUNCE_SECS: u64 = 5 * 60;
/// Coalesce the host + mine startup pulses emitted by `grid node`.
const STARTUP_DEBOUNCE_SECS: u64 = 15;

static LAST_PING_UNIX: AtomicU64 = AtomicU64::new(0);

const HEARTBEAT_VERSION: u8 = 1;
const HEARTBEAT_DOMAIN: &str = "GRID-MESH-HEARTBEAT-V1";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GlobePingBody {
    pub version: u8,
    pub public_key: String,
    pub issued_at_ms: u64,
    pub nonce: String,
    pub label: String,
    pub class: String,
    pub region: String,
    pub status: String,
    pub lat_e4: i64,
    pub lng_e4: i64,
    pub signature: String,
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

fn now_unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis().min(u128::from(u64::MAX)) as u64)
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

fn heartbeat_key_path(config_dir: &Path) -> PathBuf {
    config_dir.join("keys").join("mesh-heartbeat.key")
}

#[cfg(unix)]
fn create_secret(path: &Path, bytes: &[u8]) -> Result<()> {
    use std::os::unix::fs::OpenOptionsExt;
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
        .with_context(|| format!("create {}", path.display()))?;
    file.write_all(bytes)?;
    file.sync_all()?;
    Ok(())
}

#[cfg(not(unix))]
fn create_secret(path: &Path, bytes: &[u8]) -> Result<()> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .with_context(|| format!("create {}", path.display()))?;
    file.write_all(bytes)?;
    file.sync_all()?;
    Ok(())
}

fn load_or_create_heartbeat_key(config_dir: &Path) -> Result<SigningKey> {
    let path = heartbeat_key_path(config_dir);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(parent, fs::Permissions::from_mode(0o700))?;
        }
    }

    if !path.exists() {
        let signing = SigningKey::generate(&mut OsRng);
        match create_secret(&path, &signing.to_bytes()) {
            Ok(()) => return Ok(signing),
            Err(error)
                if error
                    .downcast_ref::<std::io::Error>()
                    .is_some_and(|io| io.kind() == std::io::ErrorKind::AlreadyExists) =>
            {
                // A concurrent node process won creation; load the winner below.
            }
            Err(error) => return Err(error),
        }
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = fs::metadata(&path)?.permissions().mode() & 0o777;
        if mode & 0o077 != 0 {
            anyhow::bail!("{} permissions are {:o}; require 600", path.display(), mode);
        }
    }
    let raw = fs::read(&path).with_context(|| format!("read {}", path.display()))?;
    let secret: [u8; 32] = raw
        .as_slice()
        .try_into()
        .map_err(|_| anyhow::anyhow!("mesh heartbeat key must be exactly 32 bytes"))?;
    Ok(SigningKey::from_bytes(&secret))
}

fn heartbeat_config_dir() -> PathBuf {
    std::env::var_os("GRID_CONFIG_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(NodeConfig::default_dir)
}

pub fn canonical_heartbeat(body: &GlobePingBody) -> Vec<u8> {
    [
        HEARTBEAT_DOMAIN.to_string(),
        format!("publicKey={}", body.public_key),
        format!("issuedAtMs={}", body.issued_at_ms),
        format!("nonce={}", body.nonce),
        format!("label={}", body.label),
        format!("class={}", body.class),
        format!("region={}", body.region),
        format!("status={}", body.status),
        format!("latE4={}", body.lat_e4),
        format!("lngE4={}", body.lng_e4),
    ]
    .join("\n")
    .into_bytes()
}

fn signed_heartbeat_with_dir(
    config_dir: &Path,
    cfg: &NodeConfig,
    lat: f64,
    lng: f64,
    region: &str,
) -> Result<GlobePingBody> {
    let signing = load_or_create_heartbeat_key(config_dir)?;
    let public_key = hex::encode(signing.verifying_key().as_bytes());
    let mut nonce = [0u8; 16];
    OsRng.fill_bytes(&mut nonce);

    let label: String = cfg
        .name
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, ' ' | '_' | '-' | '.' | '\''))
        .take(32)
        .collect::<String>()
        .trim()
        .to_string();
    let label = if label
        .chars()
        .next()
        .is_some_and(|first| first.is_ascii_alphanumeric())
    {
        label
    } else {
        "node".into()
    };
    let region: String = region
        .chars()
        .flat_map(char::to_uppercase)
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-'))
        .take(16)
        .collect();
    let region = if region.is_empty() {
        "NA-W".into()
    } else {
        region
    };

    let mut body = GlobePingBody {
        version: HEARTBEAT_VERSION,
        public_key,
        issued_at_ms: now_unix_ms(),
        nonce: hex::encode(nonce),
        label,
        class: cfg.class.as_str().to_string(),
        region,
        status: "online".into(),
        lat_e4: (lat * 10_000.0).round() as i64,
        lng_e4: (lng * 10_000.0).round() as i64,
        signature: String::new(),
    };
    body.signature = hex::encode(signing.sign(&canonical_heartbeat(&body)).to_bytes());
    Ok(body)
}

fn signed_heartbeat(cfg: &NodeConfig, lat: f64, lng: f64, region: &str) -> Result<GlobePingBody> {
    signed_heartbeat_with_dir(&heartbeat_config_dir(), cfg, lat, lng, region)
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
    let base = base.map(normalize_base).unwrap_or_else(registry_url);
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
    let base = snap.registry.as_deref().unwrap_or(DEFAULT_REGISTRY_URL);
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
        println!(
            "Computes: {} listed — grid compute available",
            snap.computes.len()
        );
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

    // Reserve the pulse before doing network I/O. `grid node` starts host and
    // mine concurrently, and both request an initial pulse; compare_exchange
    // ensures only one wins without relying on the server-side rate limiter.
    let reserved_at = now_unix();
    let minimum_gap = if force {
        STARTUP_DEBOUNCE_SECS
    } else {
        DEBOUNCE_SECS
    };
    let previous = LAST_PING_UNIX.load(Ordering::Relaxed);
    if reserved_at.saturating_sub(previous) < minimum_gap
        || LAST_PING_UNIX
            .compare_exchange(previous, reserved_at, Ordering::AcqRel, Ordering::Relaxed)
            .is_err()
    {
        debug!("globe ping debounced");
        return;
    }

    let body = match signed_heartbeat(cfg, lat, lng, &region) {
        Ok(body) => body,
        Err(error) => {
            warn!("globe ping signing failed: {error:#}");
            return;
        }
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

    match client.post(&url).json(&body).send().await {
        Ok(res) if res.status().is_success() => {
            info!("registry ping ok → {url}");
        }
        Ok(res) => {
            let status = res.status();
            let text = res.text().await.unwrap_or_default();
            let _ = LAST_PING_UNIX.compare_exchange(
                reserved_at,
                previous,
                Ordering::AcqRel,
                Ordering::Relaxed,
            );
            warn!("registry ping failed HTTP {status}: {text}");
        }
        Err(e) => {
            let _ = LAST_PING_UNIX.compare_exchange(
                reserved_at,
                previous,
                Ordering::AcqRel,
                Ordering::Relaxed,
            );
            warn!("registry ping failed: {e}");
        }
    }
}

#[cfg(test)]
mod heartbeat_tests {
    use super::*;
    use ed25519_dalek::{Signature, Verifier};
    use tempfile::tempdir;

    #[test]
    fn heartbeat_identity_is_stable_and_signature_covers_public_fields() {
        let dir = tempdir().unwrap();
        let (cfg, _) = NodeConfig::init(
            dir.path(),
            "Test Node",
            crate::config::NodeClass::S,
            "https://c",
        )
        .unwrap();
        let body = signed_heartbeat_with_dir(dir.path(), &cfg, 40.015, -105.5, "na-w").unwrap();
        let second = load_or_create_heartbeat_key(dir.path()).unwrap();

        assert_eq!(
            body.public_key,
            hex::encode(second.verifying_key().as_bytes())
        );
        assert_eq!(body.version, 1);
        assert_eq!(body.region, "NA-W");
        assert_eq!(body.lat_e4, 400_150);
        assert_eq!(body.lng_e4, -1_055_000);

        let sig_bytes: [u8; 64] = hex::decode(&body.signature).unwrap().try_into().unwrap();
        second
            .verifying_key()
            .verify(
                &canonical_heartbeat(&body),
                &Signature::from_bytes(&sig_bytes),
            )
            .unwrap();

        let mut tampered = body.clone();
        tampered.region = "EU".into();
        assert!(second
            .verifying_key()
            .verify(
                &canonical_heartbeat(&tampered),
                &Signature::from_bytes(&sig_bytes)
            )
            .is_err());
    }
}
