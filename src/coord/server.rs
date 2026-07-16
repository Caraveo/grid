//! In-process HTTP coordinator (axum).

use std::collections::{HashMap, VecDeque};
use std::net::SocketAddr;
use std::sync::Arc;

use anyhow::Result;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};
use chrono::Utc;
use parking_lot::Mutex;
use serde::Deserialize;
use tower_http::cors::CorsLayer;
use uuid::Uuid;

use crate::executor::expected_output;
use crate::protocol::JobKind;
use crate::por::{
    allocate_inclusion, allocate_proportional, effective_score, inputs_from_jobs, split_emission,
    NodeScore,
};
use crate::protocol::{result_commitment, Job, NodeInfo};
use crate::tsl::TransactSecurityLayer;

/// Demo mint size used when crediting a verified job (Phase 1).
const DEMO_EVENT_MINT: f64 = 100.0;

#[derive(Clone)]
struct App {
    inner: Arc<Mutex<Store>>,
}

struct Store {
    jobs: HashMap<String, Job>,
    queue: VecDeque<String>,
    nodes: HashMap<String, NodeInfo>,
}

impl Store {
    fn new() -> Self {
        Self {
            jobs: HashMap::new(),
            queue: VecDeque::new(),
            nodes: HashMap::new(),
        }
    }
}

pub async fn run_coordinator(bind: &str) -> Result<()> {
    let state = App {
        inner: Arc::new(Mutex::new(Store::new())),
    };

    let app = Router::new()
        .route("/health", get(health))
        .route("/v1/jobs", post(create_job))
        .route("/v1/jobs/:id", get(get_job))
        .route("/v1/jobs/complete", post(complete_job))
        .route("/v1/nodes/heartbeat", post(heartbeat))
        .route("/v1/nodes/claim", post(claim))
        .route("/v1/stats", get(stats))
        .layer(CorsLayer::permissive())
        .with_state(state);

    let addr: SocketAddr = bind.parse()?;
    tracing::info!("GRID coordinator on http://{addr}");
    println!("GRID coordinator listening on http://{addr}");
    println!("  Bitcoin TSL: {}", TransactSecurityLayer::default().describe());
    println!("  POST /v1/jobs  ·  nodes: heartbeat / claim / complete");

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}

async fn health() -> impl IntoResponse {
    Json(serde_json::json!({
        "ok": true,
        "service": "grid-coordinator",
        "phase": 1,
        "tsl": "bitcoin",
    }))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CreateJobBody {
    kind: Option<String>,
    payload: Option<String>,
    timeout_sec: Option<u64>,
}

async fn create_job(
    State(app): State<App>,
    Json(body): Json<CreateJobBody>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let kind_s = body.kind.unwrap_or_else(|| "echo".into());
    JobKind::parse(&kind_s).map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;
    let job = Job {
        id: format!("job_{}", &Uuid::new_v4().to_string()[..8]),
        kind: kind_s,
        payload: body.payload.unwrap_or_else(|| "hello-grid".into()),
        created_at: Utc::now().to_rfc3339(),
        timeout_sec: body.timeout_sec.unwrap_or(60),
        status: "queued".into(),
        assigned_node_id: None,
        earn_credits: None,
    };
    let mut g = app.inner.lock();
    g.queue.push_back(job.id.clone());
    g.jobs.insert(job.id.clone(), job.clone());
    Ok((StatusCode::CREATED, Json(job)))
}

async fn get_job(
    State(app): State<App>,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, StatusCode> {
    let g = app.inner.lock();
    g.jobs
        .get(&id)
        .cloned()
        .map(Json)
        .ok_or(StatusCode::NOT_FOUND)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct HeartbeatBody {
    node_id: String,
    class: Option<String>,
    gpu_model: Option<String>,
    max_concurrent: Option<u32>,
    cluster_id: Option<String>,
}

async fn heartbeat(
    State(app): State<App>,
    Json(body): Json<HeartbeatBody>,
) -> impl IntoResponse {
    let mut g = app.inner.lock();
    let existing = g.nodes.get(&body.node_id).cloned();
    let rec = NodeInfo {
        node_id: body.node_id.clone(),
        class: body.class.unwrap_or_else(|| "S".into()),
        gpu_model: body.gpu_model.unwrap_or_else(|| "cpu-demo".into()),
        max_concurrent: body.max_concurrent.unwrap_or(1),
        cluster_id: body
            .cluster_id
            .unwrap_or_else(|| body.node_id.clone()),
        last_seen: Utc::now().timestamp_millis(),
        jobs_done: existing.as_ref().map(|e| e.jobs_done).unwrap_or(0),
        jobs_failed: existing.as_ref().map(|e| e.jobs_failed).unwrap_or(0),
        earn_total: existing.as_ref().map(|e| e.earn_total).unwrap_or(0.0),
    };
    g.nodes.insert(body.node_id, rec.clone());
    Json(rec)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ClaimBody {
    node_id: String,
}

async fn claim(
    State(app): State<App>,
    Json(body): Json<ClaimBody>,
) -> impl IntoResponse {
    let mut g = app.inner.lock();
    if !g.nodes.contains_key(&body.node_id) {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": "heartbeat first" })),
        )
            .into_response();
    }
    while let Some(id) = g.queue.pop_front() {
        if let Some(job) = g.jobs.get_mut(&id) {
            if job.status == "queued" {
                job.status = "assigned".into();
                job.assigned_node_id = Some(body.node_id);
                return (StatusCode::OK, Json(serde_json::json!({ "job": job }))).into_response();
            }
        }
    }
    (StatusCode::NO_CONTENT, Json(serde_json::json!({ "job": null }))).into_response()
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CompleteBody {
    job_id: String,
    node_id: String,
    ok: bool,
    output: String,
    duration_ms: u64,
}

async fn complete_job(
    State(app): State<App>,
    Json(body): Json<CompleteBody>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let mut g = app.inner.lock();

    let (kind_s, payload, assigned) = {
        let job = g
            .jobs
            .get(&body.job_id)
            .ok_or((StatusCode::BAD_REQUEST, "unknown job".into()))?;
        (
            job.kind.clone(),
            job.payload.clone(),
            job.assigned_node_id.clone(),
        )
    };

    if let Some(ref a) = assigned {
        if a != &body.node_id {
            return Err((StatusCode::FORBIDDEN, "not assignee".into()));
        }
    }

    let kind = JobKind::parse(&kind_s).map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;
    let expect = expected_output(kind, &payload);
    let verified = body.ok && body.output == expect;
    let _commit = result_commitment(
        &body.job_id,
        &body.node_id,
        body.ok,
        &body.output,
        body.duration_ms,
    );

    if let Some(node) = g.nodes.get_mut(&body.node_id) {
        if verified {
            node.jobs_done += 1;
        } else {
            node.jobs_failed += 1;
        }
    }

    let mut earn = 0.0;
    if verified {
        earn = credit_event(&mut g, &body.node_id);
        if let Some(node) = g.nodes.get_mut(&body.node_id) {
            node.earn_total += earn;
        }
    }

    let job_out = {
        let job = g
            .jobs
            .get_mut(&body.job_id)
            .ok_or((StatusCode::BAD_REQUEST, "unknown job".into()))?;
        job.status = if verified {
            "verified".into()
        } else {
            "rejected".into()
        };
        if verified {
            job.earn_credits = Some(earn);
        }
        job.clone()
    };

    Ok(Json(serde_json::json!({
        "job": job_out,
        "verified": verified,
        "earnCredits": earn,
    })))
}

/// Credit using 90/10 prop/inclusion over current node scores; pay this node its share of a small event mint.
fn credit_event(store: &mut Store, winner: &str) -> f64 {
    let scores: Vec<NodeScore> = store
        .nodes
        .values()
        .map(|n| {
            let online = Utc::now().timestamp_millis() - n.last_seen < 60_000;
            let inputs = inputs_from_jobs(n.jobs_done, n.jobs_failed, online);
            NodeScore {
                node_id: n.node_id.clone(),
                cluster_id: if n.cluster_id.is_empty() {
                    n.node_id.clone()
                } else {
                    n.cluster_id.clone()
                },
                score: effective_score(&inputs),
                class_s: n.class.eq_ignore_ascii_case("S"),
            }
        })
        .collect();

    let (prop_pool, inc_pool) = split_emission(DEMO_EVENT_MINT);
    let prop = allocate_proportional(&scores, prop_pool);
    let inc = allocate_inclusion(&scores, inc_pool);

    let mut pay = 0.0;
    if let Some((_, p)) = prop.iter().find(|(id, _)| id == winner) {
        pay += p;
    }
    if let Some((_, p)) = inc.iter().find(|(id, _)| id == winner) {
        pay += p;
    }
    // Ensure winner of a verified job gets a minimum crumb if scores are tiny
    if pay < 0.01 {
        pay = 1.0;
    }
    pay
}

async fn stats(State(app): State<App>) -> impl IntoResponse {
    let g = app.inner.lock();
    let jobs: Vec<_> = g
        .jobs
        .values()
        .map(|j| {
            serde_json::json!({
                "id": j.id,
                "status": j.status,
                "kind": j.kind,
                "earnCredits": j.earn_credits,
            })
        })
        .collect();
    let nodes: Vec<_> = g.nodes.values().cloned().collect();
    Json(serde_json::json!({
        "jobs": jobs,
        "nodes": nodes,
        "tsl": "bitcoin",
        "phase": 1,
    }))
}
