//! Paid registry activation — Cash App **$Caraveo** only.
//!
//! ```text
//! grid register fire              # start / show payment for realm
//! grid register fire --confirm    # after you paid ($5 + note)
//! grid register fire --status
//! ```
//!
//! Anti-abuse: only **active** (paid + admin-approved) names may appear on
//! registry.grid / announce capacity / complete an ember's registry leg.
//! Fee creates real work (review employment) and keeps the mesh honest.
//! Donations above the fee are welcome.

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::claim::normalize_realm;
use crate::config::NodeConfig;
use crate::mesh_ping::registry_url;

const DEFAULT_FEE: f64 = 5.0;
const DEFAULT_CASHTAG: &str = "$Caraveo";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalRegRecord {
    pub name: String,
    pub reg_id: String,
    pub payment_note: String,
    pub fee_usd: f64,
    pub cashtag: String,
    pub cash_app_url: String,
    pub status: String,
    pub created_at: String,
    pub updated_at: String,
    #[serde(default)]
    pub cash_confirm: Option<String>,
}

fn reg_dir(config_dir: &Path) -> PathBuf {
    config_dir.join("registrations")
}

fn reg_path(config_dir: &Path, name: &str) -> PathBuf {
    reg_dir(config_dir).join(format!("{name}.json"))
}

fn save_local(config_dir: &Path, rec: &LocalRegRecord) -> Result<()> {
    let dir = reg_dir(config_dir);
    std::fs::create_dir_all(&dir)?;
    let path = reg_path(config_dir, &rec.name);
    std::fs::write(&path, serde_json::to_string_pretty(rec)?)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
    }
    Ok(())
}

pub fn load_local(config_dir: &Path, name: &str) -> Option<LocalRegRecord> {
    let name = normalize_realm(name).ok()?;
    let raw = std::fs::read_to_string(reg_path(config_dir, &name)).ok()?;
    serde_json::from_str(&raw).ok()
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActivationStatus {
    pub name: String,
    pub realm: String,
    pub activated: bool,
    pub status: String,
    pub fee_usd: f64,
    pub cashtag: String,
    pub payment_note: Option<String>,
    pub cash_app_url: Option<String>,
    pub message: String,
    pub donations_note: String,
}

/// Query public registry directory / name availability for activation state.
pub async fn fetch_activation(name: &str) -> Result<ActivationStatus> {
    let name = normalize_realm(name)?;
    let realm = format!("grid://{name}.grid");
    let base = registry_url();

    // 1) Active directory entries
    if let Ok(dir) = fetch_json(&format!("{base}/api/registry")).await {
        if let Some(entries) = dir.get("entries").and_then(|e| e.as_array()) {
            if entries.iter().any(|e| {
                e.get("name")
                    .and_then(|n| n.as_str())
                    .map(|n| n.eq_ignore_ascii_case(&name))
                    .unwrap_or(false)
            }) {
                return Ok(ActivationStatus {
                    name,
                    realm,
                    activated: true,
                    status: "active".into(),
                    fee_usd: fee_from_dir(&dir),
                    cashtag: cashtag_from_dir(&dir),
                    payment_note: None,
                    cash_app_url: None,
                    message: "Registry ACTIVATED — paid + approved. Ember registry leg allowed."
                        .into(),
                    donations_note: donations_blurb(cashtag_from_dir(&dir).as_str()),
                });
            }
        }
    }

    // 2) Name availability / taken (pending or reserved)
    let avail = fetch_json(&format!(
        "{base}/api/registry/register?name={}",
        url_enc(&name)
    ))
    .await
    .unwrap_or(serde_json::json!({}));

    let fee = avail
        .get("feeUsd")
        .and_then(|v| v.as_f64())
        .unwrap_or(DEFAULT_FEE);
    let cashtag = avail
        .get("cashtag")
        .and_then(|v| v.as_str())
        .unwrap_or(DEFAULT_CASHTAG)
        .to_string();

    let available = avail
        .get("available")
        .and_then(|v| v.as_bool())
        .unwrap_or(true);
    let reason = avail
        .get("reason")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    if !available && reason == "taken" {
        // Taken but not in active directory → pending payment/review
        return Ok(ActivationStatus {
            name,
            realm,
            activated: false,
            status: "pending_or_reserved".into(),
            fee_usd: fee,
            cashtag: cashtag.clone(),
            payment_note: None,
            cash_app_url: None,
            message: format!(
                "Name reserved / pending. Pay ${fee:.0} Cash App to {cashtag} with your registration note, then confirm. Admin activates after payment (prevents abuse · creates review employment)."
            ),
            donations_note: donations_blurb(&cashtag),
        });
    }

    Ok(ActivationStatus {
        name,
        realm,
        activated: false,
        status: "unregistered".into(),
        fee_usd: fee,
        cashtag: cashtag.clone(),
        payment_note: None,
        cash_app_url: None,
        message: format!(
            "Not activated. Register + pay ${fee:.0} via Cash App to {cashtag} to unlock registry.grid."
        ),
        donations_note: donations_blurb(&cashtag),
    })
}

fn fee_from_dir(dir: &serde_json::Value) -> f64 {
    dir.get("feeUsd")
        .or_else(|| dir.pointer("/payment/feeUsd"))
        .and_then(|v| v.as_f64())
        .unwrap_or(DEFAULT_FEE)
}

fn cashtag_from_dir(dir: &serde_json::Value) -> String {
    dir.get("cashtag")
        .or_else(|| dir.pointer("/payment/cashtag"))
        .and_then(|v| v.as_str())
        .unwrap_or(DEFAULT_CASHTAG)
        .to_string()
}

fn donations_blurb(cashtag: &str) -> String {
    format!(
        "Donations accepted at {cashtag} (any amount). Registry fee is ${DEFAULT_FEE:.0} with the exact note — extras still go to {cashtag}."
    )
}

async fn fetch_json(url: &str) -> Result<serde_json::Value> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()?;
    let res = client
        .get(url)
        .header("Accept", "application/json")
        .send()
        .await
        .with_context(|| format!("GET {url}"))?;
    if !res.status().is_success() {
        bail!("HTTP {} from {url}", res.status());
    }
    Ok(res.json().await?)
}

/// Start paid registration (Cash App $Caraveo).
pub async fn start_registration(
    config_dir: &Path,
    raw_name: &str,
    label: Option<&str>,
) -> Result<LocalRegRecord> {
    let name = normalize_realm(raw_name)?;
    let base = registry_url();
    let url = format!("{base}/api/registry/register");

    let cfg_path = NodeConfig::path_in(config_dir);
    let (node_id, class, region) = if cfg_path.exists() {
        let c = NodeConfig::load(&cfg_path)?;
        let region = c
            .globe_region
            .clone()
            .filter(|s| !s.is_empty())
            .unwrap_or(c.region);
        (c.node_id, c.class.to_string(), region)
    } else {
        (format!("node_{name}"), "S".into(), "NA-W".into())
    };

    let body = serde_json::json!({
        "action": "start",
        "name": name,
        "label": label.unwrap_or(&name),
        "class": class,
        "region": region,
        "nodeId": node_id,
        "kinds": ["node", "compute"],
    });

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .build()?;
    let res = client.post(&url).json(&body).send().await?;
    let status = res.status();
    let v: serde_json::Value = res.json().await.unwrap_or(serde_json::json!({}));
    if !status.is_success() {
        let err = v
            .get("error")
            .and_then(|e| e.as_str())
            .unwrap_or("registration failed");
        // If already taken, try to show local record + pay instructions
        if status.as_u16() == 409 {
            if let Some(local) = load_local(config_dir, &name) {
                print_pay_instructions(&local);
                return Ok(local);
            }
            bail!("{err} — if you already started, pay with your note to $Caraveo then: grid register {name} --confirm");
        }
        bail!("{err}");
    }

    let reg = v
        .get("registration")
        .cloned()
        .unwrap_or(serde_json::json!({}));
    let fee = v
        .get("feeUsd")
        .or_else(|| v.pointer("/payment/feeUsd"))
        .and_then(|x| x.as_f64())
        .unwrap_or(DEFAULT_FEE);
    let cashtag = v
        .get("cashtag")
        .or_else(|| v.pointer("/payment/cashtag"))
        .and_then(|x| x.as_str())
        .unwrap_or(DEFAULT_CASHTAG)
        .to_string();
    let note = reg
        .get("paymentNote")
        .and_then(|x| x.as_str())
        .unwrap_or("")
        .to_string();
    let cash_url = v
        .get("cashAppUrl")
        .or_else(|| v.pointer("/payment/url"))
        .and_then(|x| x.as_str())
        .map(|s| s.to_string())
        .unwrap_or_else(|| {
            format!(
                "https://cash.app/{}/{:.2}?note={}",
                cashtag,
                fee,
                url_enc(&note)
            )
        });
    let reg_id = reg
        .get("id")
        .and_then(|x| x.as_str())
        .unwrap_or("")
        .to_string();
    if reg_id.is_empty() {
        bail!("registry returned no registration id");
    }

    let now = chrono::Utc::now().to_rfc3339();
    let rec = LocalRegRecord {
        name: name.clone(),
        reg_id,
        payment_note: note,
        fee_usd: fee,
        cashtag,
        cash_app_url: cash_url,
        status: reg
            .get("status")
            .and_then(|x| x.as_str())
            .unwrap_or("pending_payment")
            .to_string(),
        created_at: now.clone(),
        updated_at: now,
        cash_confirm: None,
    };
    save_local(config_dir, &rec)?;
    Ok(rec)
}

/// Confirm payment after Cash App send (moves to pending_review).
pub async fn confirm_payment(
    config_dir: &Path,
    raw_name: &str,
    cash_confirm: Option<&str>,
) -> Result<LocalRegRecord> {
    let name = normalize_realm(raw_name)?;
    let mut rec = load_local(config_dir, &name).context(
        "no local registration — run: grid register <name>  then pay $5 to $Caraveo with the note",
    )?;

    let base = registry_url();
    let url = format!("{base}/api/registry/register");
    let body = serde_json::json!({
        "action": "confirm",
        "id": rec.reg_id,
        "cashConfirm": cash_confirm.unwrap_or("paid via Cash App"),
    });

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .build()?;
    let res = client.post(&url).json(&body).send().await?;
    let status = res.status();
    let v: serde_json::Value = res.json().await.unwrap_or(serde_json::json!({}));
    if !status.is_success() {
        let err = v
            .get("error")
            .and_then(|e| e.as_str())
            .unwrap_or("confirm failed");
        bail!("{err}");
    }

    rec.status = v
        .pointer("/registration/status")
        .and_then(|x| x.as_str())
        .unwrap_or("pending_review")
        .to_string();
    rec.cash_confirm = cash_confirm.map(|s| s.to_string());
    rec.updated_at = chrono::Utc::now().to_rfc3339();
    save_local(config_dir, &rec)?;
    Ok(rec)
}

pub fn print_pay_instructions(rec: &LocalRegRecord) {
    println!();
    println!("╔══════════════════════════════════════════════════════════════╗");
    println!("║  REGISTRY ACTIVATION — Cash App only · prevents abuse      ║");
    println!("╠══════════════════════════════════════════════════════════════╣");
    println!(
        "║  Pay exactly  ${:<8.2}  to  {:<28} ║",
        rec.fee_usd, rec.cashtag
    );
    println!("║  Put this EXACT note in Cash App:                          ║");
    println!("║    {:<56} ║", trunc(&rec.payment_note, 56));
    println!("╠══════════════════════════════════════════════════════════════╣");
    println!("║  Why pay?                                                  ║");
    println!("║  · Stops name squatting / spam on registry.grid            ║");
    println!("║  · Funds human review (employment) before activation       ║");
    println!(
        "║  · Donations accepted at {} anytime              ║",
        pad_tag(&rec.cashtag)
    );
    println!("╚══════════════════════════════════════════════════════════════╝");
    println!();
    println!("  Cash App link:");
    println!("    {}", rec.cash_app_url);
    println!();
    println!("  After paying:");
    println!("    grid register {} --confirm", rec.name);
    println!("  Then wait for admin approve → status becomes active.");
    println!("  Ember registry leg unlocks only when ACTIVE.");
    println!();
}

pub fn print_activation(a: &ActivationStatus) {
    println!("Registry activation · {}", a.realm);
    println!(
        "  activated   {}",
        if a.activated {
            "YES ✓"
        } else {
            "NO — pay + approve required"
        }
    );
    println!("  status      {}", a.status);
    println!("  fee         ${:.2} USD", a.fee_usd);
    println!("  pay to      {}  (Cash App only)", a.cashtag);
    if let Some(ref n) = a.payment_note {
        println!("  note        {n}");
    }
    if let Some(ref u) = a.cash_app_url {
        println!("  pay link    {u}");
    }
    println!("  {}", a.message);
    println!("  {}", a.donations_note);
}

fn pad_tag(s: &str) -> String {
    format!("{s:<12}")
}

fn trunc(s: &str, n: usize) -> String {
    let t: String = s.chars().take(n).collect();
    if s.chars().count() > n {
        format!("{t}…")
    } else {
        t
    }
}

fn url_enc(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            'a'..='z' | 'A'..='Z' | '0'..='9' | '-' | '_' | '.' | '~' => c.to_string(),
            _ => format!("%{:02X}", c as u8),
        })
        .collect()
}

/// True if name is active on public registry directory.
pub async fn is_registry_activated(name: &str) -> bool {
    fetch_activation(name)
        .await
        .map(|a| a.activated)
        .unwrap_or(false)
}
