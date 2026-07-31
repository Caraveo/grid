//! Genesis HTTP server — read-only truth. No remote ban endpoint.

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Result;
use axum::extract::{DefaultBodyLimit, Path as AxumPath, State};
use axum::http::{header, HeaderValue, Method, StatusCode};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use tokio::sync::Mutex;
use tower_http::cors::CorsLayer;

use crate::arc_protocol::{validate_signed_send, SignedArcSend};
use crate::blockchain::{block_hash, ChainReplica};
use crate::chain::ChainState;

use super::keys::GenesisKeys;
use super::store::GenesisStore;
use super::truth::TrackedPeer;

/// Server always re-opens store from disk so CLI track/ban is visible immediately.
struct App {
    config_dir: PathBuf,
    keys: GenesisKeys,
    transition_lock: Mutex<()>,
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
        transition_lock: Mutex::new(()),
    });

    let app = Router::new()
        .route("/health", get(health))
        .route("/v1/pubkey", get(pubkey_handler))
        .route("/v1/truth", get(truth_handler))
        .route("/v1/chain", get(chain_handler))
        .route("/v1/wallet/:address", get(wallet_handler))
        .route("/v1/wallet/:address/nonce", get(wallet_nonce_handler))
        .route("/v1/transactions", post(transaction_handler))
        // Announce is a *request to be noticed* — does NOT auto-track or ban.
        // Genesis operator still must `grid genesis track` locally.
        .route("/v1/announce", post(announce_handler))
        // Browser access is limited to the published Arc web origin. Native
        // clients do not send an Origin header and are unaffected.
        .layer(
            CorsLayer::new()
                .allow_origin(HeaderValue::from_static("https://grid-compute.com"))
                .allow_methods([Method::GET, Method::POST])
                .allow_headers([header::CONTENT_TYPE]),
        )
        // Keep the application safe even if the reverse proxy is bypassed.
        .layer(DefaultBodyLimit::max(16 * 1024))
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
    println!("  GET  /v1/wallet/:address  public balance + activity");
    println!("  GET  /v1/wallet/:address/nonce  next anti-replay nonce");
    println!("  POST /v1/transactions  verify + commit signed Arc transfer");
    println!("  POST /v1/announce  peer self-report (not trusted for bans)");
    println!();

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}

async fn wallet_handler(
    State(app): State<Arc<App>>,
    AxumPath(address): AxumPath<String>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let address =
        crate::address::normalize_address(&address).map_err(|_| StatusCode::BAD_REQUEST)?;
    let state = ChainState::load(&app.config_dir).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let replica =
        ChainReplica::load(&app.config_dir).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let (chain_id, height, tip_hash) = match replica {
        Some(chain) => (
            Some(chain.chain_id.clone()),
            Some(chain.tip().height),
            Some(block_hash(chain.tip()).unwrap_or_default()),
        ),
        None => (None, None, None),
    };
    let activity = state
        .txs
        .iter()
        .rev()
        .filter(|tx| {
            tx.from.as_deref() == Some(address.as_str())
                || tx.to.as_deref() == Some(address.as_str())
        })
        .take(100)
        .cloned()
        .collect::<Vec<_>>();

    Ok(Json(serde_json::json!({
        "version": 1,
        "address": address,
        "balance": state.balance(&address),
        "nextNonce": state.next_account_nonce(&address)
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?,
        "activity": activity,
        "chainId": chain_id,
        "height": height,
        "tipHash": tip_hash,
    })))
}

async fn wallet_nonce_handler(
    State(app): State<Arc<App>>,
    AxumPath(address): AxumPath<String>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let address =
        crate::address::normalize_address(&address).map_err(|_| StatusCode::BAD_REQUEST)?;
    let state = ChainState::load(&app.config_dir).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let next_nonce = state
        .next_account_nonce(&address)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(serde_json::json!({
        "address": address,
        "nextNonce": next_nonce,
    })))
}

async fn transaction_handler(
    State(app): State<Arc<App>>,
    Json(envelope): Json<SignedArcSend>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let _guard = app.transition_lock.lock().await;
    let mut state = ChainState::load(&app.config_dir).map_err(internal_error)?;
    let mut replica = ChainReplica::load(&app.config_dir)
        .map_err(internal_error)?
        .ok_or_else(|| api_error(StatusCode::SERVICE_UNAVAILABLE, "chain is not initialized"))?;
    let expected_nonce = state
        .next_account_nonce(&envelope.intent.from)
        .map_err(bad_request)?;
    let amount =
        validate_signed_send(&envelope, &replica.chain_id, expected_nonce).map_err(bad_request)?;

    let mut next_state = state.clone();
    next_state
        .commit_account_nonce(&envelope.intent.from, envelope.intent.nonce)
        .map_err(bad_request)?;
    let tx = next_state
        .transfer(
            &envelope.intent.from,
            &envelope.intent.to,
            amount,
            if envelope.intent.memo.is_empty() {
                None
            } else {
                Some(envelope.intent.memo.clone())
            },
            Some(envelope.signature.clone()),
        )
        .map_err(bad_request)?;
    let block = replica
        .append_leader_block(&app.keys, &next_state, vec![tx.clone()])
        .map_err(internal_error)?;

    // Both values are fully validated in memory before either durable write.
    next_state.save(&app.config_dir).map_err(internal_error)?;
    replica.save(&app.config_dir).map_err(internal_error)?;
    state = next_state;

    Ok(Json(serde_json::json!({
        "ok": true,
        "transaction": tx,
        "height": block.height,
        "blockHash": block_hash(&block).unwrap_or_default(),
        "nextNonce": state.next_account_nonce(&envelope.intent.from)
            .map_err(internal_error)?,
    })))
}

fn api_error(status: StatusCode, message: impl ToString) -> (StatusCode, Json<serde_json::Value>) {
    (
        status,
        Json(serde_json::json!({
            "ok": false,
            "error": message.to_string(),
        })),
    )
}

fn bad_request(error: impl ToString) -> (StatusCode, Json<serde_json::Value>) {
    api_error(StatusCode::BAD_REQUEST, error)
}

fn internal_error(error: impl ToString) -> (StatusCode, Json<serde_json::Value>) {
    eprintln!("[genesis] transaction error: {}", error.to_string());
    api_error(
        StatusCode::INTERNAL_SERVER_ERROR,
        "internal transaction error",
    )
}

async fn health(State(app): State<Arc<App>>) -> Result<Json<serde_json::Value>, StatusCode> {
    let s = open_store(&app)?;
    let replica =
        ChainReplica::load(&app.config_dir).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
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

async fn chain_handler(State(app): State<Arc<App>>) -> Result<Json<serde_json::Value>, StatusCode> {
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
