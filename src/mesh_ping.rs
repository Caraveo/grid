//! Location-only globe ping for GSITE (`POST /api/mesh/ping`).
//!
//! Never sends IPs, ports, hostnames, wallets, or coordinator URLs.
//! Globe is opt-in: skip if lat/lng missing.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::Serialize;
use tracing::{debug, info, warn};

use crate::config::NodeConfig;

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

fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn site_url() -> Option<String> {
    std::env::var("GRID_SITE_URL")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .map(|s| {
            let s = s.trim_end_matches('/').to_string();
            if s.starts_with("http://") || s.starts_with("https://") {
                s
            } else {
                format!("https://{s}")
            }
        })
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

/// Fire-and-forget friendly: never panics; does not block mining.
/// `force` = true on node start (still respects missing coords / missing URL).
pub async fn ping_globe(cfg: &NodeConfig, force: bool) {
    let Some(base) = site_url() else {
        debug!("globe ping skipped (GRID_SITE_URL unset)");
        return;
    };

    let Some((lat, lng, region)) = resolve_coords(cfg) else {
        info!("globe ping skipped (no coords)");
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
            info!("globe ping ok → {url}");
        }
        Ok(res) => {
            let status = res.status();
            let text = res.text().await.unwrap_or_default();
            warn!("globe ping failed HTTP {status}: {text}");
        }
        Err(e) => {
            warn!("globe ping failed: {e}");
        }
    }
}
