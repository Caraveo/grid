//! **Ember** = host + mine + compute + **registry** for a realm (`grid://name.grid`).
//!
//! ```text
//! grid ember fire           # status of the fire.grid ember
//! grid ember fire --start   # announce + run host+mine for that compute
//! ```
//!
//! One ember is the full stack for a named realm:
//!   compute + host + mine + **paid** public registry
//!
//! Registry leg requires Cash App activation ($5 → `$Caraveo` + note → approve).
//! That fee prevents abuse and funds human review employment. Donations accepted.

use anyhow::{bail, Result};
use serde::Serialize;
use std::path::Path;
use std::process::Command;
use std::time::Duration;

use crate::claim::{self, normalize_realm};
use crate::compute::{announce_computes, fetch_computes, load_manifest, load_status};
use crate::config::NodeConfig;
use crate::coord::CoordinatorClient;
use crate::mesh_ping::registry_url;
use crate::register;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EmberStatus {
    pub name: String,
    pub realm: String,
    /// host + mine + compute + registry all green
    pub ready: bool,
    pub can_start: bool,
    pub checklist: EmberChecklist,
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EmberChecklist {
    pub compute: bool,
    pub compute_state: String,
    pub replicas: u32,
    pub free_slots: u32,
    pub content: bool,
    pub content_url: Option<String>,
    pub coordinator: bool,
    pub coordinator_url: String,
    pub host_mine_daemon: bool,
    /// Paid + approved on registry.grid (Cash App $Caraveo)
    pub registry_activated: bool,
    /// Live announce heartbeat visible on computes API
    pub registry_announced: bool,
    pub registry_url: String,
    pub registry_status: String,
    pub registry_free_slots: u32,
    pub fee_usd: f64,
    pub cashtag: String,
    pub realm_claimed: bool,
    pub vault_unlocked: bool,
}

/// Probe whether content origin answers (MESH names or default fire port).
fn content_alive(name: &str, config_dir: &Path) -> (bool, Option<String>) {
    let names = config_dir.join("browser").join("names.toml");
    let mut origin: Option<String> = None;
    if names.exists() {
        if let Ok(raw) = std::fs::read_to_string(&names) {
            let key = format!("{name} =");
            for line in raw.lines() {
                let t = line.trim();
                if t.starts_with(&key) || t.starts_with(&format!("{name}=")) {
                    if let Some((_, v)) = t.split_once('=') {
                        let v = v.trim().trim_matches('"').trim_matches('\'');
                        if !v.is_empty() {
                            origin = Some(v.to_string());
                        }
                    }
                }
            }
        }
    }
    if origin.is_none() && name == "fire" {
        origin = Some("http://127.0.0.1:8080".into());
    }
    let Some(url) = origin else {
        return (false, None);
    };
    let health = if url.ends_with('/') {
        format!("{url}health")
    } else {
        format!("{url}/health")
    };
    let ok = Command::new("curl")
        .args(["-sf", "-m", "2", &health])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
        || Command::new("curl")
            .args(["-sf", "-m", "2", "-o", "/dev/null", &url])
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
    (ok, Some(url))
}

fn daemon_running() -> bool {
    let out = Command::new("pgrep").args(["-f", "grid node"]).output();
    if let Ok(o) = out {
        if !o.stdout.is_empty() {
            return true;
        }
    }
    let out = Command::new("pgrep").args(["-f", "grid host"]).output();
    out.map(|o| !o.stdout.is_empty()).unwrap_or(false)
}

/// Look up this compute name on the public registry.
async fn registry_presence(name: &str) -> (bool, String, u32) {
    let base = registry_url();
    match tokio::time::timeout(Duration::from_secs(6), fetch_computes(None, false)).await {
        Ok(Ok(snap)) => {
            if let Some(c) = snap
                .computes
                .iter()
                .find(|c| c.name.eq_ignore_ascii_case(name))
            {
                let online = c.status == "available" || c.status == "busy";
                let detail = format!(
                    "{} · {} · slots {}/{} · node {}",
                    c.status,
                    base,
                    c.free_slots,
                    c.replicas,
                    c.node_id.chars().take(14).collect::<String>()
                );
                (online, detail, c.free_slots)
            } else {
                (
                    false,
                    format!("not listed on {base} — grid compute announce"),
                    0,
                )
            }
        }
        Ok(Err(e)) => (false, format!("registry error: {e}"), 0),
        Err(_) => (false, format!("registry timeout ({base})"), 0),
    }
}

pub async fn status(config_dir: &Path, raw: &str) -> Result<EmberStatus> {
    let name = normalize_realm(raw)?;
    let realm = format!("grid://{name}.grid");

    let mut compute_ok = false;
    let mut compute_state = "missing".into();
    let mut replicas = 0u32;
    let mut free_slots = 0u32;

    if let Ok(m) = load_manifest(config_dir, &name) {
        replicas = m.replicas;
        if let Ok(st) = load_status(config_dir, &name) {
            compute_state = st.state.clone();
            compute_ok = st.state == "ready" || st.state == "running" || st.state == "registered";
            free_slots = if compute_ok { m.replicas } else { 0 };
        } else {
            compute_state = "no-status".into();
        }
    }

    let (content_ok, content_url) = content_alive(&name, config_dir);

    let cfg_path = NodeConfig::path_in(config_dir);
    let (coordinator_url, node_id, node_label) = if cfg_path.exists() {
        match NodeConfig::load(&cfg_path) {
            Ok(c) => (c.coordinator, c.node_id, c.name),
            Err(_) => (
                "http://127.0.0.1:9876".into(),
                "node_local".into(),
                "local".into(),
            ),
        }
    } else {
        (
            "http://127.0.0.1:9876".into(),
            "node_local".into(),
            "local".into(),
        )
    };

    let client = CoordinatorClient::new(&coordinator_url);
    let coordinator_ok = match tokio::time::timeout(Duration::from_secs(2), client.health()).await {
        Ok(Ok(true)) => true,
        Ok(Ok(false)) => false,
        _ => match tokio::time::timeout(Duration::from_secs(2), client.stats()).await {
            Ok(Ok(_)) => true,
            _ => false,
        },
    };

    let host_mine = daemon_running();
    let (announced, announce_detail, registry_free) = registry_presence(&name).await;
    let activation = register::fetch_activation(&name)
        .await
        .unwrap_or(register::ActivationStatus {
            name: name.clone(),
            realm: realm.clone(),
            activated: false,
            status: "unknown".into(),
            fee_usd: 5.0,
            cashtag: "$Caraveo".into(),
            payment_note: None,
            cash_app_url: None,
            message: "could not reach registry".into(),
            donations_note: "Donations accepted at $Caraveo.".into(),
        });
    let activated = activation.activated;
    let reg_url = registry_url();
    // Registry leg of ember = paid activation (required) + optional live announce
    let registry_ok = activated;
    let registry_status = if activated && announced {
        format!("ACTIVATED + live · {announce_detail}")
    } else if activated {
        format!(
            "ACTIVATED (paid) · not announced yet — grid compute announce · {}",
            activation.cashtag
        )
    } else {
        format!(
            "NOT ACTIVATED — pay ${:.0} Cash App to {} with note · {}",
            activation.fee_usd, activation.cashtag, activation.status
        )
    };

    let realm_claimed = claim::load_claim(config_dir, &name).is_ok();
    let vault = crate::passkey::auth_status(config_dir);
    let vault_unlocked = vault.session_unlocked || vault.mode == "nocrypt";

    // Can start host/mine when local capacity + coord exist
    let can_start = compute_ok && coordinator_ok;
    // Full ember ready: compute + content + coord + host/mine + paid registry
    let ready = can_start && content_ok && host_mine && registry_ok;

    let message = if ready {
        format!("EMBER READY — {realm} · host + mine + compute + registry (paid)")
    } else if can_start && host_mine && !activated {
        format!(
            "Pay ${:.0} Cash App to {} to activate registry: grid register {name}",
            activation.fee_usd, activation.cashtag
        )
    } else if can_start && host_mine && activated && !announced {
        format!("Activated — announce capacity: grid compute announce  (or grid ember {name} --start)")
    } else if can_start && host_mine {
        format!("Ember running (host+mine) — finish checklist for {realm}")
    } else if can_start {
        format!("You can start this ember: grid ember {name} --start")
    } else if !compute_ok {
        format!("Launch compute first: grid launch {name} --public")
    } else if !coordinator_ok {
        "Start coordinator: grid coord  (or grid-fabric start)".into()
    } else {
        format!("Not ready for ember on {realm}")
    };

    let _ = (node_id, node_label);

    Ok(EmberStatus {
        name,
        realm,
        ready,
        can_start,
        checklist: EmberChecklist {
            compute: compute_ok,
            compute_state,
            replicas,
            free_slots,
            content: content_ok,
            content_url,
            coordinator: coordinator_ok,
            coordinator_url,
            host_mine_daemon: host_mine,
            registry_activated: activated,
            registry_announced: announced,
            registry_url: reg_url,
            registry_status,
            registry_free_slots: registry_free,
            fee_usd: activation.fee_usd,
            cashtag: activation.cashtag,
            realm_claimed,
            vault_unlocked,
        },
        message,
    })
}

pub fn print_status(s: &EmberStatus) {
    println!("EMBER  ·  host + mine + compute + registry");
    println!("  realm       {}", s.realm);
    println!("  ready       {}", if s.ready { "YES ✓" } else { "not yet" });
    println!("  can_start   {}", if s.can_start { "YES" } else { "no" });
    println!();
    println!("  Checklist");
    let c = &s.checklist;
    println!(
        "    [{}] compute     {}  replicas={}/{}  state={}",
        on(c.compute),
        s.name,
        c.free_slots,
        c.replicas,
        c.compute_state
    );
    println!(
        "    [{}] content     {}",
        on(c.content),
        c.content_url.as_deref().unwrap_or("(no origin mapped)")
    );
    println!(
        "    [{}] coordinator {}",
        on(c.coordinator),
        c.coordinator_url
    );
    println!(
        "    [{}] host+mine   {}",
        on(c.host_mine_daemon),
        if c.host_mine_daemon {
            "daemon running"
        } else {
            "not running"
        }
    );
    println!(
        "    [{}] registry    {}  (paid activation required)",
        on(c.registry_activated),
        c.registry_status
    );
    if c.registry_activated {
        println!(
            "    [{}] announced  live slots on registry",
            on(c.registry_announced)
        );
    }
    println!(
        "    [{}] claim       {}",
        on(c.realm_claimed),
        if c.realm_claimed {
            "realm claimed"
        } else {
            "optional — grid claim <name>"
        }
    );
    println!(
        "    [{}] vault       {}",
        on(c.vault_unlocked),
        if c.vault_unlocked {
            "unlocked"
        } else {
            "locked (claim needs: grid auth login)"
        }
    );
    println!();
    println!("  {}", s.message);
    if !c.registry_activated {
        println!();
        println!("  ── Registry paywall (anti-abuse · review jobs) ──");
        println!(
            "  Send ${:.0} via Cash App to {} with your registration note.",
            c.fee_usd, c.cashtag
        );
        println!("  Donations accepted at {} anytime.", c.cashtag);
        println!("    grid register {}", s.name);
        println!("    grid register {} --confirm   # after payment", s.name);
    }
    if s.can_start && c.registry_activated && (!c.host_mine_daemon || !c.registry_announced) {
        println!();
        println!("  Start / re-announce:");
        println!("    grid ember {} --start", s.name);
        println!("    grid compute announce");
        println!("    grid-fabric start");
    }
    if s.ready {
        println!();
        println!("  Open:      {}", s.realm);
        println!("  Registry:  {}/api/registry/computes", c.registry_url);
        println!("  Host:      grid host --compute {}", s.name);
        println!("  Both:      grid node");
    }
}

fn on(b: bool) -> char {
    if b {
        'x'
    } else {
        ' '
    }
}

/// Announce to registry, then run host+mine for this compute (full ember start).
pub async fn start_ember(config_dir: &Path, raw: &str, cfg: NodeConfig) -> Result<()> {
    let name = normalize_realm(raw)?;
    let st = status(config_dir, &name).await?;
    if !st.can_start {
        print_status(&st);
        bail!("cannot start ember — fix checklist items first");
    }

    println!("Starting EMBER for {} …", st.realm);
    println!("  tracks:   HOST + MINE + COMPUTE + REGISTRY");
    println!("  compute:  {name}");
    println!();

    if !st.checklist.registry_activated {
        println!("✗ Registry not activated (paid).");
        println!(
            "  Pay ${:.0} Cash App to {} — prevents abuse, funds review employment.",
            st.checklist.fee_usd, st.checklist.cashtag
        );
        println!("  Donations accepted at {}.", st.checklist.cashtag);
        println!("    grid register {name}");
        println!("    grid register {name} --confirm");
        bail!("registry activation required before ember registry leg");
    }

    // Registry leg — re-announce so the mesh sees free slots (server rejects unpaid names)
    println!("→ registry announce (activated name)…");
    announce_computes(config_dir, &cfg.node_id, &cfg.name).await;
    println!("  announced → {}", registry_url());

    // Confirm presence
    let (reg_ok, reg_detail, _) = registry_presence(&name).await;
    if reg_ok {
        println!("  registry: {reg_detail}");
    } else {
        println!("  registry: not listed yet ({reg_detail})");
        println!("            (will retry via host heartbeats / grid compute announce)");
    }
    println!();

    std::env::set_var("GRID_CONFIG_DIR", config_dir);
    crate::node::run_ember(cfg, Some(name)).await
}
