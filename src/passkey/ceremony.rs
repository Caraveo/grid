//! Local-host WebAuthn ceremony (browser → iCloud / platform passkey).

use std::sync::Arc;
use std::time::Duration;

use anyhow::{bail, Context, Result};
use axum::extract::State;
use axum::response::Html;
use axum::routing::{get, post};
use axum::{Json, Router};
use chrono::Utc;
use parking_lot::Mutex;
use tokio::sync::oneshot;
use url::Url;
use webauthn_rs::prelude::*;

use super::store::{self, PasskeyStore};

const RP_ID: &str = "localhost";
const RP_NAME: &str = "GRID Operator";

struct SharedReg {
    webauthn: Webauthn,
    reg_state: Mutex<Option<PasskeyRegistration>>,
    done: Mutex<Option<oneshot::Sender<Result<Passkey>>>>,
}

struct SharedAuth {
    webauthn: Webauthn,
    auth_state: Mutex<Option<PasskeyAuthentication>>,
    done: Mutex<Option<oneshot::Sender<Result<()>>>>,
}

fn webauthn_for(origin_port: u16) -> Result<Webauthn> {
    let origin = Url::parse(&format!("http://localhost:{origin_port}"))?;
    // localhost is a valid secure context for WebAuthn in browsers
    let builder = WebauthnBuilder::new(RP_ID, &origin)?.rp_name(RP_NAME);
    Ok(builder.build()?)
}

/// Open system browser to URL (macOS `open`, Linux `xdg-open`, etc.).
fn open_browser(url: &str) -> Result<()> {
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg(url)
            .spawn()
            .context("open browser")?;
        return Ok(());
    }
    #[cfg(target_os = "linux")]
    {
        std::process::Command::new("xdg-open")
            .arg(url)
            .spawn()
            .context("xdg-open")?;
        return Ok(());
    }
    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("cmd")
            .args(["/C", "start", url])
            .spawn()
            .context("start browser")?;
        return Ok(());
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    {
        println!("Open this URL in your browser:\n  {url}");
        Ok(())
    }
}

fn page_register(options_json: &str) -> String {
    format!(
        r#"<!DOCTYPE html>
<html><head><meta charset="utf-8"/><title>GRID Passkey</title>
<style>
  body {{ font-family: system-ui,sans-serif; background:#0a0a0a; color:#eee;
         display:flex; min-height:100vh; align-items:center; justify-content:center; }}
  .card {{ max-width:28rem; padding:2rem; border:1px solid #333; border-radius:12px; }}
  button {{ background:#fff; color:#000; border:0; padding:.75rem 1.25rem; font-weight:600;
            border-radius:8px; cursor:pointer; width:100%; }}
  pre {{ font-size:12px; color:#888; white-space:pre-wrap; }}
</style></head>
<body><div class="card">
  <h1>Register GRID passkey</h1>
  <p>Use your device / <strong>iCloud Keychain</strong> passkey when prompted.</p>
  <button id="go">Create passkey</button>
  <pre id="log"></pre>
</div>
<script>
const opts = {options_json};
function b64urlToBuf(s) {{
  s = s.replace(/-/g,'+').replace(/_/g,'/');
  const pad = s.length % 4 === 0 ? '' : '='.repeat(4 - (s.length % 4));
  const bin = atob(s + pad);
  const buf = new Uint8Array(bin.length);
  for (let i=0;i<bin.length;i++) buf[i]=bin.charCodeAt(i);
  return buf.buffer;
}}
function bufToB64url(buf) {{
  const bytes = new Uint8Array(buf);
  let s=''; for (const b of bytes) s+=String.fromCharCode(b);
  return btoa(s).replace(/\+/g,'-').replace(/\//g,'_').replace(/=+$/,'');
}}
function prepCreate(o) {{
  o.publicKey.challenge = b64urlToBuf(o.publicKey.challenge);
  o.publicKey.user.id = b64urlToBuf(o.publicKey.user.id);
  if (o.publicKey.excludeCredentials) {{
    o.publicKey.excludeCredentials = o.publicKey.excludeCredentials.map(c => ({{
      ...c, id: b64urlToBuf(c.id)
    }}));
  }}
  return o;
}}
document.getElementById('go').onclick = async () => {{
  const log = (m) => document.getElementById('log').textContent = m;
  try {{
    const cred = await navigator.credentials.create(prepCreate(structuredClone(opts)));
    const body = {{
      id: cred.id,
      rawId: bufToB64url(cred.rawId),
      type: cred.type,
      response: {{
        clientDataJSON: bufToB64url(cred.response.clientDataJSON),
        attestationObject: bufToB64url(cred.response.attestationObject),
      }},
    }};
    const res = await fetch('/finish', {{ method:'POST', headers:{{'content-type':'application/json'}}, body: JSON.stringify(body) }});
    const t = await res.text();
    log(res.ok ? '✓ Passkey registered. You can close this window.' : t);
  }} catch (e) {{ log(String(e)); }}
}};
</script></body></html>"#,
        options_json = options_json
    )
}

fn page_assert(options_json: &str, purpose: &str) -> String {
    let purpose = purpose.replace('<', "").replace('>', "");
    format!(
        r#"<!DOCTYPE html>
<html><head><meta charset="utf-8"/><title>GRID Passkey</title>
<style>
  body {{ font-family: system-ui,sans-serif; background:#0a0a0a; color:#eee;
         display:flex; min-height:100vh; align-items:center; justify-content:center; }}
  .card {{ max-width:28rem; padding:2rem; border:1px solid #333; border-radius:12px; }}
  button {{ background:#fff; color:#000; border:0; padding:.75rem 1.25rem; font-weight:600;
            border-radius:8px; cursor:pointer; width:100%; }}
  pre {{ font-size:12px; color:#888; white-space:pre-wrap; }}
</style></head>
<body><div class="card">
  <h1>Verify passkey</h1>
  <p>Authorize: <strong>{purpose}</strong></p>
  <p>Use your <strong>iCloud / device passkey</strong> when prompted.</p>
  <button id="go">Continue with passkey</button>
  <pre id="log"></pre>
</div>
<script>
const opts = {options_json};
function b64urlToBuf(s) {{
  s = s.replace(/-/g,'+').replace(/_/g,'/');
  const pad = s.length % 4 === 0 ? '' : '='.repeat(4 - (s.length % 4));
  const bin = atob(s + pad);
  const buf = new Uint8Array(bin.length);
  for (let i=0;i<bin.length;i++) buf[i]=bin.charCodeAt(i);
  return buf.buffer;
}}
function bufToB64url(buf) {{
  const bytes = new Uint8Array(buf);
  let s=''; for (const b of bytes) s+=String.fromCharCode(b);
  return btoa(s).replace(/\+/g,'-').replace(/\//g,'_').replace(/=+$/,'');
}}
function prepGet(o) {{
  o.publicKey.challenge = b64urlToBuf(o.publicKey.challenge);
  if (o.publicKey.allowCredentials) {{
    o.publicKey.allowCredentials = o.publicKey.allowCredentials.map(c => ({{
      ...c, id: b64urlToBuf(c.id)
    }}));
  }}
  return o;
}}
document.getElementById('go').onclick = async () => {{
  const log = (m) => document.getElementById('log').textContent = m;
  try {{
    const cred = await navigator.credentials.get(prepGet(structuredClone(opts)));
    const body = {{
      id: cred.id,
      rawId: bufToB64url(cred.rawId),
      type: cred.type,
      response: {{
        clientDataJSON: bufToB64url(cred.response.clientDataJSON),
        authenticatorData: bufToB64url(cred.response.authenticatorData),
        signature: bufToB64url(cred.response.signature),
        userHandle: cred.response.userHandle ? bufToB64url(cred.response.userHandle) : null,
      }},
    }};
    const res = await fetch('/finish', {{ method:'POST', headers:{{'content-type':'application/json'}}, body: JSON.stringify(body) }});
    const t = await res.text();
    log(res.ok ? '✓ Verified. You can close this window.' : t);
  }} catch (e) {{ log(String(e)); }}
}};
// auto-start
document.getElementById('go').click();
</script></body></html>"#,
        options_json = options_json,
        purpose = purpose
    )
}

/// Register a platform/iCloud passkey (browser ceremony).
pub async fn register_passkey(config_dir: &std::path::Path) -> Result<()> {
    if store::has_passkey(config_dir) {
        bail!("passkey already registered — delete ~/.grid/passkey to reset");
    }

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let port = listener.local_addr()?.port();
    let webauthn = webauthn_for(port)?;

    let user_name = "grid-operator".to_string();
    let user_unique = Uuid::new_v4();
    let (ccr, reg_state) =
        webauthn.start_passkey_registration(user_unique, &user_name, &user_name, None)?;

    let (tx, rx) = oneshot::channel();
    let shared = Arc::new(SharedReg {
        webauthn,
        reg_state: Mutex::new(Some(reg_state)),
        done: Mutex::new(Some(tx)),
    });

    let options_json = serde_json::to_string(&ccr)?;
    let html = page_register(&options_json);

    let app = Router::new()
        .route("/", get(move || async move { Html(html) }))
        .route("/finish", post(finish_register))
        .with_state(shared.clone());

    let server = axum::serve(listener, app);
    let url = format!("http://localhost:{port}/");
    println!("Opening browser for passkey registration…");
    println!("  {url}");
    println!("  (Use iCloud Keychain / Touch ID / device passkey when prompted)\n");
    open_browser(&url)?;

    let server_task = tokio::spawn(async move {
        let _ = server.await;
    });

    let passkey = tokio::time::timeout(Duration::from_secs(180), rx)
        .await
        .context("passkey registration timed out (3 min)")?
        .context("registration cancelled")??;

    server_task.abort();

    store::save(
        config_dir,
        &PasskeyStore {
            rp_id: RP_ID.into(),
            user_name,
            passkey,
            registered_at: Utc::now().to_rfc3339(),
        },
    )?;
    println!("✓ Passkey registered under {}", store::store_path(config_dir).display());
    Ok(())
}

async fn finish_register(
    State(st): State<Arc<SharedReg>>,
    Json(body): Json<serde_json::Value>,
) -> Result<String, (axum::http::StatusCode, String)> {
    let state = st
        .reg_state
        .lock()
        .take()
        .ok_or((axum::http::StatusCode::BAD_REQUEST, "no reg state".into()))?;

    let reg: RegisterPublicKeyCredential = serde_json::from_value(body).map_err(|e| {
        (
            axum::http::StatusCode::BAD_REQUEST,
            format!("bad credential: {e}"),
        )
    })?;

    let passkey = st
        .webauthn
        .finish_passkey_registration(&reg, &state)
        .map_err(|e| {
            (
                axum::http::StatusCode::BAD_REQUEST,
                format!("verify failed: {e}"),
            )
        })?;

    if let Some(tx) = st.done.lock().take() {
        let _ = tx.send(Ok(passkey));
    }
    Ok("ok".into())
}

/// Require a successful passkey assertion (opens browser / system UI).
pub async fn require_passkey(config_dir: &std::path::Path, purpose: &str) -> Result<()> {
    let stored = store::load(config_dir)?.context("no passkey — run: grid passkey register")?;

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let port = listener.local_addr()?.port();
    let webauthn = webauthn_for(port)?;

    let keys = vec![stored.passkey.clone()];
    let (rcr, auth_state) = webauthn.start_passkey_authentication(&keys)?;

    let (tx, rx) = oneshot::channel();
    let shared = Arc::new(SharedAuth {
        webauthn,
        auth_state: Mutex::new(Some(auth_state)),
        done: Mutex::new(Some(tx)),
    });

    let options_json = serde_json::to_string(&rcr)?;
    let html = page_assert(&options_json, purpose);

    let app = Router::new()
        .route("/", get(move || async move { Html(html) }))
        .route("/finish", post(finish_assert))
        .with_state(shared.clone());

    let server = axum::serve(listener, app);
    let url = format!("http://localhost:{port}/");
    println!("Passkey required: {purpose}");
    println!("  Opening {url}");
    open_browser(&url)?;

    let server_task = tokio::spawn(async move {
        let _ = server.await;
    });

    tokio::time::timeout(Duration::from_secs(120), rx)
        .await
        .context("passkey verification timed out")?
        .context("verification cancelled")??;

    server_task.abort();
    println!("✓ Passkey verified");
    Ok(())
}

async fn finish_assert(
    State(st): State<Arc<SharedAuth>>,
    Json(body): Json<serde_json::Value>,
) -> Result<String, (axum::http::StatusCode, String)> {
    let state = st
        .auth_state
        .lock()
        .take()
        .ok_or((axum::http::StatusCode::BAD_REQUEST, "no auth state".into()))?;

    let auth: PublicKeyCredential = serde_json::from_value(body).map_err(|e| {
        (
            axum::http::StatusCode::BAD_REQUEST,
            format!("bad assertion: {e}"),
        )
    })?;

    st.webauthn
        .finish_passkey_authentication(&auth, &state)
        .map_err(|e| {
            (
                axum::http::StatusCode::BAD_REQUEST,
                format!("verify failed: {e}"),
            )
        })?;

    if let Some(tx) = st.done.lock().take() {
        let _ = tx.send(Ok(()));
    }
    Ok("ok".into())
}
