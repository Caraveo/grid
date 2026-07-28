//! Genesis HTTP server — read-only truth. No remote ban endpoint.

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Result;
use axum::extract::State;
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use tower_http::cors::CorsLayer;

use crate::blockchain::{block_hash, ChainReplica};

use super::keys::GenesisKeys;
use super::store::GenesisStore;
use super::truth::TrackedPeer;

/// Server always re-opens store from disk so CLI track/ban is visible immediately.
struct App {
    config_dir: PathBuf,
    keys: GenesisKeys,
}

fn open_store(app: &App) -> Result<GenesisStore, StatusCode> {
    GenesisStore::open_with_keys(&app.config_dir, app.keys.clone())
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

pub async fn run_genesis_server(config_dir: PathBuf, bind: &str, keys: GenesisKeys) -> Result<()> {
    let store = GenesisStore::open_with_keys(&config_dir, keys.clone())?;
    let pubkey = store.keys().public_hex();
    let epoch = store.epoch();

    let app_state = Arc::new(App {
        config_dir: config_dir.clone(),
        keys,
    });

    let app = Router::new()
        .route("/health", get(health))
        .route("/v1/pubkey", get(pubkey_handler))
        .route("/v1/truth", get(truth_handler))
        .route("/v1/chain", get(chain_handler))
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
    println!("  GET  /v1/truth   signed snapshot (reloads from disk each request)");
    println!("  GET  /v1/pubkey  genesis public key");
    println!("  POST /v1/announce  peer self-report (not trusted for bans)");
    println!();

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}

async fn health(State(app): State<Arc<App>>) -> Result<Json<serde_json::Value>, StatusCode> {
    let s = open_store(&app)?;
    let replica = ChainReplica::load(&app.config_dir)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let chain = replica.as_ref().map(|chain| {
        let tip = chain.tip();
        serde_json::json!({
            "id": chain.chain_id,
            "height": tip.height,
            "tipHash": block_hash(tip).unwrap_or_default(),
            "leaderPubkey": chain.leader_pubkey,
            "maxSupply": chain.max_supply,
            "blocks": chain.blocks.len(),
        })
    });
    Ok(Json(serde_json::json!({
        "ok": true,
        "role": "genesis",
        "phase": 0,
        "epoch": s.epoch(),
        "tracked": s.list_tracked().len(),
        "banned": s.list_banned().len(),
        "tsl": "bitcoin",
        "authority": ["track_peers", "ban_peers"],
        "chain": chain,
    })))
}

async fn chain_handler(
    State(app): State<Arc<App>>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let chain = ChainReplica::load(&app.config_dir)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::NOT_FOUND)?;
    let blocks = chain
        .blocks
        .iter()
        .rev()
        .take(25)
        .map(|block| {
            serde_json::json!({
                "height": block.height,
                "hash": block_hash(block).unwrap_or_default(),
                "previousHash": block.previous_hash,
                "timestamp": block.timestamp,
                "stateRoot": block.state_root,
                "transactions": block.transactions.len(),
                "settlements": block.settlements.len(),
            })
        })
        .collect::<Vec<_>>();
    Ok(Json(serde_json::json!({
        "chainId": chain.chain_id,
        "leaderPubkey": chain.leader_pubkey,
        "maxSupply": chain.max_supply,
        "height": chain.tip().height,
        "tipHash": block_hash(chain.tip()).unwrap_or_default(),
        "recoveryKeys": chain.recovery_pubkeys.len(),
        "blocks": blocks,
    })))
}

async fn pubkey_handler(
    State(app): State<Arc<App>>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let s = open_store(&app)?;
    Ok(Json(serde_json::json!({
        "genesis_pubkey": s.keys().public_hex(),
    })))
}

async fn truth_handler(State(app): State<Arc<App>>) -> Result<Json<serde_json::Value>, StatusCode> {
    let s = open_store(&app)?;
    let snap = s
        .snapshot()
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
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
