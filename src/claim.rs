//! Realm claims — bind a `grid://name.grid` identity to this operator.
//!
//! ```text
//! grid claim fire              # claim realm (passkey + operator Ed25519)
//! grid claim fire.grid
//! grid claim list
//! grid claim status fire
//! ```
//!
//! Security:
//! 1. Vault unlock (operator auth when configured)
//! 2. Fresh passkey ceremony (step-up IdentityKey)
//! 3. Ed25519 signature over canonical claim body with operator key
//! 4. Local persistence + optional public registry POST

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;
use tracing::{info, warn};

use crate::compute::{list_computes, machine_id};
use crate::config::NodeConfig;
use crate::mesh_ping::{normalize_base, registry_url};
use crate::passkey::{operator_pubkey_hex, require_identity, sign_operator, verify_operator_sig};

const CLAIM_DOMAIN: &str = "GRID realm claim v1";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RealmClaim {
    /// Canonical realm label (no scheme / .grid)
    pub name: String,
    /// mesh address form
    pub realm: String,
    pub operator_pubkey: String,
    pub node_id: String,
    pub node_label: String,
    pub machine_id: String,
    #[serde(default)]
    pub class: String,
    #[serde(default)]
    pub region: String,
    /// Local computes bound at claim time
    #[serde(default)]
    pub computes: Vec<String>,
    pub claimed_at: String,
    /// blake3 of canonical body (pre-signature)
    pub body_hash: String,
    /// Ed25519 signature hex over body_hash bytes (UTF-8 hex string of hash)
    pub signature: String,
    /// How the operator authenticated for this claim
    pub auth: ClaimAuth,
    /// Registry POST outcome (best-effort)
    #[serde(default)]
    pub registry: Option<ClaimRegistryMeta>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClaimAuth {
    pub mode: String,
    pub passkey: bool,
    pub session_step_up: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClaimRegistryMeta {
    pub url: String,
    pub status: String,
    #[serde(default)]
    pub message: Option<String>,
    #[serde(default)]
    pub registered: Option<bool>,
}

fn claims_dir(config_dir: &Path) -> PathBuf {
    config_dir.join("claims")
}

fn claim_path(config_dir: &Path, name: &str) -> PathBuf {
    claims_dir(config_dir).join(format!("{name}.json"))
}

/// Normalize `grid://fire.grid`, `fire.grid`, `FIRE` → `fire`
pub fn normalize_realm(raw: &str) -> Result<String> {
    let mut s = raw.trim().to_lowercase();
    if s.is_empty() {
        bail!("realm name required");
    }
    if let Some(rest) = s.strip_prefix("grid://") {
        s = rest.to_string();
    }
    if let Some(rest) = s.strip_prefix("grid:") {
        s = rest.trim_start_matches('/').to_string();
    }
    // path leftovers
    if let Some((head, _)) = s.split_once('/') {
        s = head.to_string();
    }
    if let Some(rest) = s.strip_suffix(".grid") {
        s = rest.to_string();
    }
    s = s
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
        .collect();
    if s.len() < 2 || s.len() > 32 {
        bail!("realm name must be 2–32 chars (a-z 0-9 _ -)");
    }
    let reserved = [
        "genesis", "home", "registry", "grid", "start", "newtab", "mesh", "peers", "computes",
        "status", "about", "help", "docs", "settings", "config", "prefs", "error", "www", "api",
        "admin",
    ];
    if reserved.contains(&s.as_str()) {
        bail!("reserved realm name '{s}'");
    }
    Ok(s)
}

/// Canonical bytes that are hashed then signed.
fn claim_body_bytes(
    name: &str,
    operator_pubkey: &str,
    node_id: &str,
    machine_id: &str,
    claimed_at: &str,
    computes: &[String],
) -> Vec<u8> {
    // Stable, human-auditable lines — order fixed.
    let mut comps = computes.to_vec();
    comps.sort();
    let mut s = String::new();
    s.push_str(CLAIM_DOMAIN);
    s.push('\n');
    s.push_str("name=");
    s.push_str(name);
    s.push('\n');
    s.push_str("operator=");
    s.push_str(operator_pubkey);
    s.push('\n');
    s.push_str("node=");
    s.push_str(node_id);
    s.push('\n');
    s.push_str("machine=");
    s.push_str(machine_id);
    s.push('\n');
    s.push_str("at=");
    s.push_str(claimed_at);
    s.push('\n');
    s.push_str("computes=");
    s.push_str(&comps.join(","));
    s.push('\n');
    s.into_bytes()
}

pub fn load_claim(config_dir: &Path, name: &str) -> Result<RealmClaim> {
    let name = normalize_realm(name)?;
    let path = claim_path(config_dir, &name);
    let raw = fs::read_to_string(&path)
        .with_context(|| format!("no local claim for '{name}' — grid claim {name}"))?;
    let claim: RealmClaim = serde_json::from_str(&raw)?;
    verify_claim(&claim)?;
    Ok(claim)
}

pub fn list_claims(config_dir: &Path) -> Result<Vec<RealmClaim>> {
    let dir = claims_dir(config_dir);
    if !dir.exists() {
        return Ok(vec![]);
    }
    let mut out = Vec::new();
    for ent in fs::read_dir(&dir)? {
        let ent = ent?;
        let p = ent.path();
        if p.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        if let Ok(raw) = fs::read_to_string(&p) {
            if let Ok(c) = serde_json::from_str::<RealmClaim>(&raw) {
                if verify_claim(&c).is_ok() {
                    out.push(c);
                }
            }
        }
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(out)
}

pub fn verify_claim(claim: &RealmClaim) -> Result<()> {
    let body = claim_body_bytes(
        &claim.name,
        &claim.operator_pubkey,
        &claim.node_id,
        &claim.machine_id,
        &claim.claimed_at,
        &claim.computes,
    );
    let hash = crate::crypto::blake3_hex(&body);
    if hash != claim.body_hash {
        bail!("claim body_hash mismatch (tampered?)");
    }
    // Sign the hash string (hex) for compact signatures
    verify_operator_sig(
        &claim.operator_pubkey,
        claim.body_hash.as_bytes(),
        &claim.signature,
    )?;
    Ok(())
}

/// Claim a realm with IdentityKey (passkey) + operator Ed25519.
pub async fn claim_realm(config_dir: &Path, raw_name: &str) -> Result<RealmClaim> {
    let name = normalize_realm(raw_name)?;
    let realm = format!("grid://{name}.grid");

    println!("GRID claim — secure realm binding");
    println!("  realm     {realm}");
    println!("  security  vault unlock → passkey IdentityKey → Ed25519 sign");
    println!();

    // 1–2: unlock + step-up passkey
    let dek = require_identity(config_dir, &format!("Claim realm {realm}")).await?;
    let operator_pubkey = operator_pubkey_hex(config_dir)?;

    // Node identity
    let cfg_path = NodeConfig::path_in(config_dir);
    let (node_id, node_label, class, region) = if cfg_path.exists() {
        let c = NodeConfig::load(&cfg_path)?;
        let region = c
            .globe_region
            .clone()
            .filter(|s| !s.is_empty())
            .unwrap_or(c.region);
        (c.node_id, c.name, c.class.to_string(), region)
    } else {
        (format!("node_{name}"), name.clone(), "S".into(), "—".into())
    };
    let mid = machine_id(config_dir).unwrap_or_else(|_| "mach_unknown".into());
    let computes: Vec<String> = list_computes(config_dir)
        .unwrap_or_default()
        .into_iter()
        .map(|m| m.name)
        .collect();

    let claimed_at = chrono::Utc::now().to_rfc3339();
    let body = claim_body_bytes(
        &name,
        &operator_pubkey,
        &node_id,
        &mid,
        &claimed_at,
        &computes,
    );
    let body_hash = crate::crypto::blake3_hex(&body);
    let signature = sign_operator(config_dir, &dek, body_hash.as_bytes())?;

    let st = crate::passkey::auth_status(config_dir);
    let mut claim = RealmClaim {
        name: name.clone(),
        realm: realm.clone(),
        operator_pubkey: operator_pubkey.clone(),
        node_id: node_id.clone(),
        node_label,
        machine_id: mid,
        class,
        region,
        computes,
        claimed_at,
        body_hash,
        signature,
        auth: ClaimAuth {
            mode: st.mode,
            passkey: st.passkey_registered,
            session_step_up: true,
        },
        registry: None,
    };

    // Self-verify before persist
    verify_claim(&claim)?;

    // Persist local
    let dir = claims_dir(config_dir);
    fs::create_dir_all(&dir)?;
    let path = claim_path(config_dir, &name);
    fs::write(&path, serde_json::to_string_pretty(&claim)?)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(&path, fs::Permissions::from_mode(0o600));
    }

    // Best-effort registry POST
    match post_claim_to_registry(&claim).await {
        Ok(meta) => {
            claim.registry = Some(meta);
            fs::write(&path, serde_json::to_string_pretty(&claim)?)?;
        }
        Err(e) => {
            warn!("registry claim POST failed (local claim still valid): {e}");
            claim.registry = Some(ClaimRegistryMeta {
                url: format!("{}/api/registry/claim", registry_url()),
                status: "local_only".into(),
                message: Some(e.to_string()),
                registered: None,
            });
            fs::write(&path, serde_json::to_string_pretty(&claim)?)?;
        }
    }

    // Wire MESH names if content origin known
    ensure_browser_name(config_dir, &name)?;

    // Re-announce computes so registry sees this host as owner of name
    crate::compute::announce_computes(config_dir, &node_id, &claim.node_label).await;

    info!("realm claimed: {realm}");
    Ok(claim)
}

async fn post_claim_to_registry(claim: &RealmClaim) -> Result<ClaimRegistryMeta> {
    let base = registry_url();
    let url = format!("{base}/api/registry/claim");
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(12))
        .build()
        .context("http client")?;

    let body = serde_json::json!({
        "name": claim.name,
        "realm": claim.realm,
        "operatorPubkey": claim.operator_pubkey,
        "nodeId": claim.node_id,
        "nodeLabel": claim.node_label,
        "machineId": claim.machine_id,
        "class": claim.class,
        "region": claim.region,
        "computes": claim.computes,
        "claimedAt": claim.claimed_at,
        "bodyHash": claim.body_hash,
        "signature": claim.signature,
        "auth": {
            "mode": claim.auth.mode,
            "passkey": claim.auth.passkey,
            "sessionStepUp": claim.auth.session_step_up,
        },
    });

    let mut req = client.post(&url).json(&body);
    if let Ok(secret) = std::env::var("GRID_WEBHOOK_SECRET") {
        let secret = secret.trim();
        if !secret.is_empty() {
            req = req.header("Authorization", format!("Bearer {secret}"));
        }
    }

    let res = req.send().await.with_context(|| format!("POST {url}"))?;
    let status = res.status();
    let text = res.text().await.unwrap_or_default();
    let registered = serde_json::from_str::<serde_json::Value>(&text)
        .ok()
        .and_then(|v| {
            v.get("ok")
                .and_then(|x| x.as_bool())
                .or_else(|| v.get("registered").and_then(|x| x.as_bool()))
        });

    if status.is_success() {
        Ok(ClaimRegistryMeta {
            url,
            status: "accepted".into(),
            message: Some(text.chars().take(200).collect()),
            registered,
        })
    } else if status.as_u16() == 404 {
        Ok(ClaimRegistryMeta {
            url,
            status: "endpoint_missing".into(),
            message: Some("registry has no /api/registry/claim yet — local claim kept".into()),
            registered: Some(false),
        })
    } else {
        bail!(
            "HTTP {status}: {}",
            text.chars().take(240).collect::<String>()
        );
    }
}

/// Ensure `~/.grid/browser/names.toml` has an entry when we host content for this realm.
fn ensure_browser_name(config_dir: &Path, name: &str) -> Result<()> {
    // Prefer existing names.toml under config or default home .grid
    let browser_dir = config_dir.join("browser");
    let names_path = browser_dir.join("names.toml");
    fs::create_dir_all(&browser_dir)?;

    // fire default content port
    let origin = if name == "fire" {
        "http://127.0.0.1:8080".to_string()
    } else {
        // leave as registry-only unless already mapped
        if names_path.exists() {
            let raw = fs::read_to_string(&names_path)?;
            if raw.contains(&format!("{name} =")) {
                return Ok(());
            }
        }
        return Ok(());
    };

    let mut map = if names_path.exists() {
        fs::read_to_string(&names_path)?
    } else {
        "# MESH local name map\n[names]\n".into()
    };
    if !map.contains("[names]") {
        map.push_str("\n[names]\n");
    }
    let key = format!("{name} =");
    if map.lines().any(|l| l.trim_start().starts_with(&key)) {
        // replace line
        let mut out = String::new();
        for line in map.lines() {
            if line.trim_start().starts_with(&key) {
                out.push_str(&format!("{name} = \"{origin}\"\n"));
            } else {
                out.push_str(line);
                out.push('\n');
            }
        }
        fs::write(&names_path, out)?;
    } else {
        if !map.ends_with('\n') {
            map.push('\n');
        }
        map.push_str(&format!("{name} = \"{origin}\"\n"));
        fs::write(&names_path, map)?;
    }
    Ok(())
}

pub fn print_claim(claim: &RealmClaim) {
    println!("Realm claim");
    println!("  realm       {}", claim.realm);
    println!("  name        {}", claim.name);
    println!(
        "  operator    {}…",
        &claim.operator_pubkey[..16.min(claim.operator_pubkey.len())]
    );
    println!("  node        {} ({})", claim.node_label, claim.node_id);
    println!("  machine     {}", claim.machine_id);
    println!("  class       {}", claim.class);
    println!("  region      {}", claim.region);
    println!(
        "  computes    {}",
        if claim.computes.is_empty() {
            "(none)".into()
        } else {
            claim.computes.join(", ")
        }
    );
    println!("  claimed_at  {}", claim.claimed_at);
    println!("  body_hash   {}", claim.body_hash);
    println!(
        "  signature   {}…",
        &claim.signature[..16.min(claim.signature.len())]
    );
    println!(
        "  auth        mode={} passkey={} step_up={}",
        claim.auth.mode, claim.auth.passkey, claim.auth.session_step_up
    );
    if let Some(ref r) = claim.registry {
        println!("  registry    {} ({})", r.status, r.url);
        if let Some(ref m) = r.message {
            println!("              {}", m.chars().take(120).collect::<String>());
        }
    }
    println!("  verify      OK (Ed25519 + body hash)");
}

pub fn print_list(config_dir: &Path) -> Result<()> {
    let items = list_claims(config_dir)?;
    if items.is_empty() {
        println!("No local realm claims.");
        println!("  grid claim fire          # claim grid://fire.grid");
        println!("  grid claim my-realm");
        return Ok(());
    }
    println!("{:14} {:28} {:10} {}", "NAME", "REALM", "AUTH", "CLAIMED");
    for c in items {
        println!(
            "{:14} {:28} {:10} {}",
            c.name,
            c.realm,
            c.auth.mode,
            &c.claimed_at[..19.min(c.claimed_at.len())]
        );
    }
    Ok(())
}

/// Check public registry name availability (informational).
pub async fn check_registry_name(name: &str) -> Result<serde_json::Value> {
    let base = registry_url();
    let url = format!(
        "{}/api/registry/register?name={}",
        normalize_base(&base),
        urlencoding_lite(name)
    );
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(8))
        .build()?;
    let res = client.get(&url).send().await?;
    let v: serde_json::Value = res.json().await.unwrap_or(serde_json::json!({}));
    Ok(v)
}

fn urlencoding_lite(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            'a'..='z' | 'A'..='Z' | '0'..='9' | '-' | '_' | '.' | '~' => c.to_string(),
            _ => format!("%{:02X}", c as u8),
        })
        .collect()
}
