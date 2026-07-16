//! Genesis HTTP server — read-only truth. No remote ban endpoint.

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Result;
use axum::extract::State;
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Json, Router};
use parking_lot::Mutex;
use serde::Deserialize;
use tower_http::cors::CorsLayer;

use super::store::GenesisStore;
use super::truth::TrackedPeer;

struct App {
    store: Mutex<GenesisStore>,
}

pub async fn run_genesis_server(config_dir: PathBuf, bind: &str) -> Result<()> {
    let store = GenesisStore::open(&config_dir)?;
    let pubkey = store.keys().public_hex();
    let epoch = store.epoch();

    let app_state = Arc::new(App {
        store: Mutex::new(store),
    });

    let app = Router::new()
        .route("/health", get(health))
        .route("/v1/pubkey", get(pubkey_handler))
        .route("/v1/truth", get(truth_handler))
        // Announce is a *request to be noticed* — does NOT auto-track or ban.
        // Genesis operator still must `grid genesis track` locally.
        .route("/v1/announce", post(announce_handler))
        .layer(CorsLayer::permissive())
        .with_state(app_state);

    let addr: SocketAddr = bind.parse()?;
    println!("GRID GENESIS authority (Phase 0)");
    println!("  bind     http://{addr}");
    println!("  pubkey   {pubkey}");
    println!("  epoch    {epoch}");
    println!("  scope    track peers + ban peers ONLY");
    println!("  security secret key never leaves this host; no remote ban API");
    println!();
    println!("  GET  /v1/truth   signed snapshot");
    println!("  GET  /v1/pubkey  genesis public key");
    println!("  POST /v1/announce  peer self-report (not trusted for bans)");
    println!();

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}

async fn health(State(app): State<Arc<App>>) -> Json<serde_json::Value> {
    let s = app.store.lock();
    Json(serde_json::json!({
        "ok": true,
        "role": "genesis",
        "phase": 0,
        "epoch": s.epoch(),
        "tracked": s.list_tracked().len(),
        "banned": s.list_banned().len(),
        "tsl": "bitcoin",
        "authority": ["track_peers", "ban_peers"],
    }))
}

async fn pubkey_handler(State(app): State<Arc<App>>) -> Json<serde_json::Value> {
    let s = app.store.lock();
    Json(serde_json::json!({
        "genesis_pubkey": s.keys().public_hex(),
    }))
}

async fn truth_handler(
    State(app): State<Arc<App>>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let s = app.store.lock();
    let snap = s.snapshot().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(serde_json::to_value(snap).unwrap()))
}

#[derive(Deserialize)]
struct AnnounceBody {
    peer_id: String,
    name: Option<String>,
    listen: Option<String>,
    class: Option<String>,
}

/// Log announce only — never mutates ban list; never auto-trusts.
async fn announce_handler(
    State(_app): State<Arc<App>>,
    Json(body): Json<AnnounceBody>,
) -> Json<serde_json::Value> {
    println!(
        "[genesis] announce peer_id={} name={} listen={} class={} (not auto-tracked)",
        body.peer_id,
        body.name.as_deref().unwrap_or("-"),
        body.listen.as_deref().unwrap_or("-"),
        body.class.as_deref().unwrap_or("-"),
    );
    let _hint = TrackedPeer {
        peer_id: body.peer_id.clone(),
        name: body.name.unwrap_or_default(),
        listen: body.listen.unwrap_or_default(),
        class: body.class.unwrap_or_else(|| "S".into()),
        tracked_at: String::new(),
    };
    Json(serde_json::json!({
        "ok": true,
        "accepted": "announce_logged",
        "note": "Genesis must run: grid genesis track — bans never apply from announce",
    }))
}
