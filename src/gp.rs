//! GRID Protocol identity + consent-gated compliance (local helpers).
//!
//! Human UI: `grid://{realm}.grid` only.
//! Internal locator: `gp://{128-hex}` — never print on public marketing paths.
//!
//! Identity formula matches GProc crate:
//! `blake3_xof64("GRID-GP-v1" || 0x00 || pubkey32 || 0x00 || realm)`.

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

use crate::claim::normalize_realm;
use crate::compute::machine_id;
use crate::config::NodeConfig;
use crate::mesh_ping::registry_url;
use crate::passkey::{operator_pubkey_hex, require_unlocked, sign_operator};

pub const GP_ID_DOMAIN: &[u8] = b"GRID-GP-v1";
pub const GP_ID_HEX_LEN: usize = 128;
pub const CONSENT_VERSION: &str = "2026-07-consent-v1";

pub const CONSENT_TEXT: &str = r#"GRID compliance collection (optional)

If you enable this, your node may send network identifiers (IP address and
hardware/network adapter reference) to the GRID registry operators for
abuse response, legal compliance, and forensics.

• This is OFF by default.
• Public mesh pages and the Mesh browser never show your IP or MAC.
• Only authenticated registry admins can decrypt this data after step-up auth.
• You can disable collection anytime: grid compliance disable
• Your mesh identity does not include your machine or IP.

By enabling, you consent to this collection while it remains enabled.
"#;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ComplianceLocal {
    pub enabled: bool,
    pub consent_version: String,
    pub consented_at: String,
    #[serde(default)]
    pub last_push_at: Option<String>,
}

fn compliance_path(config_dir: &Path) -> PathBuf {
    config_dir.join("compliance.json")
}

pub fn load_compliance(config_dir: &Path) -> ComplianceLocal {
    let p = compliance_path(config_dir);
    if let Ok(raw) = fs::read_to_string(&p) {
        if let Ok(c) = serde_json::from_str(&raw) {
            return c;
        }
    }
    ComplianceLocal {
        enabled: false,
        consent_version: String::new(),
        consented_at: String::new(),
        last_push_at: None,
    }
}

fn save_compliance(config_dir: &Path, c: &ComplianceLocal) -> Result<()> {
    fs::create_dir_all(config_dir)?;
    let p = compliance_path(config_dir);
    fs::write(&p, serde_json::to_string_pretty(c)?)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(&p, fs::Permissions::from_mode(0o600));
    }
    Ok(())
}

/// Print consent and enable local flag (interactive yes).
pub fn compliance_enable(config_dir: &Path, yes: bool) -> Result<ComplianceLocal> {
    println!("{CONSENT_TEXT}");
    if !yes {
        eprint!("Type yes to consent and enable: ");
        let mut line = String::new();
        std::io::stdin().read_line(&mut line)?;
        if line.trim().eq_ignore_ascii_case("yes") {
            // ok
        } else {
            bail!("consent not given — compliance remains disabled");
        }
    }
    let c = ComplianceLocal {
        enabled: true,
        consent_version: CONSENT_VERSION.into(),
        consented_at: chrono::Utc::now().to_rfc3339(),
        last_push_at: load_compliance(config_dir).last_push_at,
    };
    save_compliance(config_dir, &c)?;
    println!("✓ Compliance collection ENABLED (consent recorded)");
    println!("  Push: grid compliance push <realm>");
    println!("  Off:  grid compliance disable");
    Ok(c)
}

pub fn compliance_disable(config_dir: &Path) -> Result<()> {
    let mut c = load_compliance(config_dir);
    c.enabled = false;
    save_compliance(config_dir, &c)?;
    println!("✓ Compliance collection DISABLED");
    Ok(())
}

pub fn compliance_status(config_dir: &Path) -> ComplianceLocal {
    load_compliance(config_dir)
}

/// Derive 128-hex GP id from operator Ed25519 pubkey + realm.
pub fn gp_id_hex(pubkey_bytes: &[u8], realm: &str) -> Result<String> {
    if pubkey_bytes.len() != 32 {
        bail!("pubkey must be 32 bytes");
    }
    let realm = normalize_realm(realm)?;
    let mut hasher = blake3::Hasher::new();
    hasher.update(GP_ID_DOMAIN);
    hasher.update(&[0u8]);
    hasher.update(pubkey_bytes);
    hasher.update(&[0u8]);
    hasher.update(realm.as_bytes());
    let mut out = [0u8; 64];
    hasher.finalize_xof().fill(&mut out);
    Ok(hex::encode(out))
}

pub fn gp_id_for_operator(config_dir: &Path, realm: &str) -> Result<String> {
    let hex_pk = operator_pubkey_hex(config_dir)?;
    let bytes = hex::decode(hex_pk.trim()).context("decode operator pubkey")?;
    gp_id_hex(&bytes, realm)
}

/// Best-effort local MAC / adapter reference (forensic, not auth).
fn local_mac_ref() -> String {
    // Stable machine ref already exists; MAC is OS-specific — prefer hashed placeholder.
    #[cfg(target_os = "macos")]
    {
        if let Ok(out) = std::process::Command::new("ifconfig").output() {
            let s = String::from_utf8_lossy(&out.stdout);
            for line in s.lines() {
                if let Some(idx) = line.find("ether ") {
                    let mac = line[idx + 6..].trim().split_whitespace().next().unwrap_or("");
                    if mac.len() >= 11 {
                        return mac.to_lowercase();
                    }
                }
            }
        }
    }
    #[cfg(target_os = "linux")]
    {
        if let Ok(entries) = fs::read_dir("/sys/class/net") {
            for e in entries.flatten() {
                let name = e.file_name().to_string_lossy().to_string();
                if name == "lo" {
                    continue;
                }
                let p = e.path().join("address");
                if let Ok(mac) = fs::read_to_string(p) {
                    let mac = mac.trim().to_lowercase();
                    if mac.len() >= 11 && mac != "00:00:00:00:00:00" {
                        return mac;
                    }
                }
            }
        }
    }
    "mac_unavailable".into()
}

fn local_ip_ref() -> String {
    // Outbound-facing guess via UDP trick
    if let Ok(socket) = std::net::UdpSocket::bind("0.0.0.0:0") {
        if socket.connect("8.8.8.8:80").is_ok() {
            if let Ok(addr) = socket.local_addr() {
                return addr.ip().to_string();
            }
        }
    }
    "0.0.0.0".into()
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PushResult {
    pub ok: bool,
    pub gp_id: String,
    pub realm: String,
    pub message: String,
}

/// Push encrypted-at-rest attestation to registry (requires consent).
pub async fn compliance_push(config_dir: &Path, realm: &str) -> Result<PushResult> {
    let local = load_compliance(config_dir);
    if !local.enabled {
        bail!("compliance disabled — run: grid compliance enable");
    }
    if local.consent_version != CONSENT_VERSION {
        bail!("consent version outdated — run: grid compliance enable");
    }

    let realm = normalize_realm(realm)?;
    let cfg = NodeConfig::load(&NodeConfig::path_in(config_dir))?;
    let gp_id = gp_id_for_operator(config_dir, &realm)?;
    let machine_ref = machine_id(config_dir).unwrap_or_else(|_| "mach_unknown".into());
    let ip = local_ip_ref();
    let mac = local_mac_ref();
    let collected_at = chrono::Utc::now().to_rfc3339();

    let body_str = format!(
        "GRID-GP-compliance-v1|{gp_id}|{realm}|{}|{machine_ref}|{ip}|{mac}|{collected_at}|{CONSENT_VERSION}",
        cfg.node_id
    );
    let body_hash = hex::encode(blake3::hash(body_str.as_bytes()).as_bytes());

    let signature = match require_unlocked(config_dir, "compliance push").await {
        Ok(dek) => sign_operator(config_dir, &dek, body_hash.as_bytes()).unwrap_or_default(),
        Err(_) => String::new(),
    };

    let base = registry_url();
    let url = format!("{base}/api/registry/compliance");
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()?;

    let mut req = client.post(&url).json(&serde_json::json!({
        "consent": true,
        "consentVersion": CONSENT_VERSION,
        "gpId": gp_id,
        "realm": realm,
        "nodeId": cfg.node_id,
        "machineRef": machine_ref,
        "ip": ip,
        "mac": mac,
        "collectedAt": collected_at,
        "bodyHash": body_hash,
        "signature": signature,
    }));

    if let Ok(sec) = std::env::var("GRID_MESH_WEBHOOK_SECRET") {
        if !sec.is_empty() {
            req = req.header("x-grid-webhook-secret", sec);
        }
    }

    let res = req.send().await.context("compliance POST")?;
    let status = res.status();
    let text = res.text().await.unwrap_or_default();
    if !status.is_success() {
        bail!("compliance push failed HTTP {status}: {text}");
    }

    let mut c = load_compliance(config_dir);
    c.last_push_at = Some(collected_at);
    save_compliance(config_dir, &c)?;

    Ok(PushResult {
        ok: true,
        gp_id,
        realm,
        message: "attestation accepted (encrypted at rest on registry)".into(),
    })
}

pub fn print_gp_id(config_dir: &Path, realm: &str, json: bool) -> Result<()> {
    let realm_n = normalize_realm(realm)?;
    let id = gp_id_for_operator(config_dir, &realm_n)?;
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "realm": realm_n,
                "gridUrl": format!("grid://{realm_n}.grid"),
                "gpId": id,
                // wire form internal only — still available in JSON for tooling
                "wire": format!("gp://{id}"),
            }))?
        );
    } else {
        println!("realm    {realm_n}");
        println!("grid     grid://{realm_n}.grid");
        println!("id       {id}");
        println!("(Mesh address bar shows grid:// only)");
    }
    Ok(())
}

/// Announce this node's P2P listen to the registry dial directory.
pub async fn p2p_announce(config_dir: &Path, realm: &str, listen: &str) -> Result<()> {
    let realm = normalize_realm(realm)?;
    let cfg = NodeConfig::load(&NodeConfig::path_in(config_dir))?;
    let gp_id = gp_id_for_operator(config_dir, &realm)?;
    let base = registry_url();
    let url = format!("{base}/api/registry/p2p");
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(20))
        .build()?;
    let mut req = client.post(&url).json(&serde_json::json!({
        "gpId": gp_id,
        "realm": realm,
        "nodeId": cfg.node_id,
        "label": cfg.name,
        "listen": listen,
        "class": cfg.class.as_str(),
    }));
    if let Ok(sec) = std::env::var("GRID_MESH_WEBHOOK_SECRET") {
        if !sec.is_empty() {
            req = req.header("x-grid-webhook-secret", sec);
        }
    }
    let res = req.send().await.context("p2p announce")?;
    let status = res.status();
    let text = res.text().await.unwrap_or_default();
    if !status.is_success() {
        bail!("p2p announce HTTP {status}: {text}");
    }
    println!("✓ P2P dial announce for grid://{realm}.grid @ {listen}");
    Ok(())
}

/// Resolve listen multiaddrs for a realm or 128-hex id via registry.
pub async fn resolve_dial(query: &str) -> Result<Vec<(String, String, String)>> {
    let base = registry_url();
    let q = query.trim();
    let url = if q.len() == GP_ID_HEX_LEN && q.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
    {
        format!("{base}/api/registry/p2p?gpId={q}")
    } else {
        let realm = normalize_realm(q)?;
        format!("{base}/api/registry/p2p?realm={realm}")
    };
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()?;
    let res = client.get(&url).send().await.context("p2p resolve")?;
    let v: serde_json::Value = res.json().await.context("p2p resolve json")?;
    let peers = v
        .get("peers")
        .and_then(|p| p.as_array())
        .cloned()
        .unwrap_or_default();
    let mut out = Vec::new();
    for p in peers {
        let listen = p
            .get("listen")
            .and_then(|x| x.as_str())
            .unwrap_or("")
            .to_string();
        let realm = p
            .get("realm")
            .and_then(|x| x.as_str())
            .unwrap_or("")
            .to_string();
        let gp = p
            .get("gpId")
            .and_then(|x| x.as_str())
            .unwrap_or("")
            .to_string();
        if !listen.is_empty() {
            out.push((gp, realm, listen));
        }
    }
    Ok(out)
}

/// One-shot TCP dial to resolved peer (connectivity check).
pub async fn dial_once(query: &str) -> Result<()> {
    let peers = resolve_dial(query).await?;
    if peers.is_empty() {
        bail!("no P2P dial targets for «{query}» — peer must announce (grid peer --realm …)");
    }
    for (gp, realm, listen) in &peers {
        print!("dial {listen} (grid://{realm}.grid) … ");
        match tokio::time::timeout(
            std::time::Duration::from_secs(5),
            tokio::net::TcpStream::connect(listen.as_str()),
        )
        .await
        {
            Ok(Ok(_stream)) => {
                println!("ok");
                println!("  id  {}…", &gp[..gp.len().min(16)]);
                return Ok(());
            }
            Ok(Err(e)) => println!("fail ({e})"),
            Err(_) => println!("timeout"),
        }
    }
    bail!("could not connect to any dial target");
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteCert {
    pub version: u32,
    pub gp_id: String,
    pub realm: String,
    pub pubkey_hex: String,
    pub tier: String,
    #[serde(default)]
    pub entity_name: Option<String>,
    pub issued_at: String,
    pub not_after: String,
    pub ca_signature: String,
    #[serde(default)]
    pub payment_ref: Option<String>,
    #[serde(default)]
    pub ca_pubkey_hex: Option<String>,
}

fn cert_canonical(c: &RemoteCert) -> String {
    let entity = c.entity_name.as_deref().unwrap_or("");
    let pay = c.payment_ref.as_deref().unwrap_or("");
    format!(
        "GRID-GP-CERT-v{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}",
        c.version,
        c.gp_id,
        c.realm,
        c.pubkey_hex.to_lowercase(),
        c.tier.to_lowercase(),
        entity,
        c.issued_at,
        c.not_after,
        pay,
    )
}

/// Fetch + verify permanent cert for a realm from registry.
pub async fn verify_remote_cert(realm: &str) -> Result<RemoteCert> {
    let realm = normalize_realm(realm)?;
    let base = registry_url();
    let url = format!("{base}/api/registry/entity?realm={realm}&cert=1");
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()?;
    let res = client.get(&url).send().await.context("fetch cert")?;
    let v: serde_json::Value = res.json().await?;
    if !v.get("certActive").and_then(|x| x.as_bool()).unwrap_or(false) {
        bail!("no active permanent cert for grid://{realm}.grid");
    }
    let cert: RemoteCert = serde_json::from_value(
        v.get("cert")
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("missing cert"))?,
    )?;
    // Expiry
    if let Ok(exp) = chrono::DateTime::parse_from_rfc3339(&cert.not_after) {
        if exp.with_timezone(&chrono::Utc) < chrono::Utc::now() {
            bail!("cert expired");
        }
    }
    // Signature via ed25519 if ca pubkey present
    if let Some(ref ca_pk) = cert.ca_pubkey_hex {
        let pk_bytes = hex::decode(ca_pk).context("ca pubkey")?;
        let arr: [u8; 32] = pk_bytes
            .as_slice()
            .try_into()
            .map_err(|_| anyhow::anyhow!("ca pubkey must be 32 bytes"))?;
        let vk = ed25519_dalek::VerifyingKey::from_bytes(&arr)?;
        let sig_bytes = hex::decode(&cert.ca_signature).context("ca sig")?;
        let sig_arr: [u8; 64] = sig_bytes
            .as_slice()
            .try_into()
            .map_err(|_| anyhow::anyhow!("sig must be 64 bytes"))?;
        let sig = ed25519_dalek::Signature::from_bytes(&sig_arr);
        let body = cert_canonical(&cert);
        use ed25519_dalek::Verifier;
        vk.verify(body.as_bytes(), &sig)
            .map_err(|e| anyhow::anyhow!("invalid CA signature: {e}"))?;
    } else {
        bail!("cert missing caPubkeyHex");
    }
    Ok(cert)
}

pub fn print_cert(c: &RemoteCert) {
    println!("✓ Permanent cert valid");
    println!("  realm   grid://{}.grid", c.realm);
    println!("  tier    [{}]", c.tier);
    println!("  id      {}…", &c.gp_id[..c.gp_id.len().min(20)]);
    println!("  issued  {}", c.issued_at);
    println!("  until   {}", c.not_after);
    if let Some(ref n) = c.entity_name {
        println!("  entity  {n}");
    }
}
