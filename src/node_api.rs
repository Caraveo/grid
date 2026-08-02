//! Read-only HTTP status exposed by `grid node` for local and remote wallets.

use std::net::SocketAddr;
use std::path::PathBuf;

use anyhow::{Context, Result};
use axum::{extract::State, routing::get, Json, Router};
use tower_http::cors::CorsLayer;

use crate::blockchain::{block_hash, ChainReplica};

#[derive(Clone)]
struct App {
    config_dir: PathBuf,
    node_id: String,
    p2p_listen: String,
}

pub async fn serve(
    config_dir: PathBuf,
    node_id: String,
    p2p_listen: String,
    bind: String,
) -> Result<()> {
    let addr: SocketAddr = bind
        .parse()
        .with_context(|| format!("bad --wallet-bind {bind}"))?;
    let app = Router::new()
        .route("/health", get(health))
        .layer(CorsLayer::permissive())
        .with_state(App {
            config_dir,
            node_id,
            p2p_listen,
        });
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .with_context(|| format!("bind wallet API on {addr}"))?;
    println!("GRID wallet API listening on http://{addr}");
    axum::serve(listener, app).await?;
    Ok(())
}

async fn health(State(app): State<App>) -> Json<serde_json::Value> {
    let chain = ChainReplica::load(&app.config_dir)
        .ok()
        .flatten()
        .map(|chain| {
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
    Json(serde_json::json!({
        "ok": true,
        "role": "node",
        "nodeId": app.node_id,
        "chain": chain,
        "p2p": {
            "listen": app.p2p_listen,
            "protocol": crate::p2p::PROTOCOL,
        },
    }))
}
