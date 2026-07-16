//! Persistent HTTP coordinator — real pilot fabric (not a throwaway demo).
//!
//! * Jobs, nodes, and earn ledger survive restarts (`~/.grid/coord/state.json`)
//! * Auto-work emits verifiable `blake3_work` PoR jobs
//! * Verification re-executes the allowlisted function server-side

use std::collections::{HashMap, VecDeque};
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use axum::extract::{Path as AxumPath, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};
use chrono::Utc;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use tower_http::cors::CorsLayer;
use uuid::Uuid;

use crate::earn::EarnLedger;
use crate::executor::{expected_output, fabric_work_payload, DEFAULT_BLAKE3_ITERS};
use crate::por::{
    allocate_inclusion, allocate_proportional, effective_score, inputs_from_jobs, split_emission,
    NodeScore,
};
use crate::protocol::{result_commitment, Job, JobKind, NodeInfo};
use crate::tsl::TransactSecurityLayer;

/// Credits allocated per verified PoR event (off-chain pilot ledger).
const POR_EVENT_MINT: f64 = 100.0;
/// Keep the queue fed so miners always have real work.
const AUTO_WORK_TARGET_QUEUED: usize = 8;
const AUTO_WORK_ITERS: u64 = DEFAULT_BLAKE3_ITERS;

#[derive(Clone)]
struct App {
    inner: Arc<Mutex<Store>>,
    data_dir: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PersistedState {
    jobs: HashMap<String, Job>,
    queue: VecDeque<String>,
    nodes: HashMap<String, NodeInfo>,
    earn: EarnLedger,
    work_seq: u64,
}

struct Store {
    jobs: HashMap<String, Job>,
    queue: VecDeque<String>,
    nodes: HashMap<String, NodeInfo>,
    earn: EarnLedger,
    work_seq: u64,
    dirty: bool,
}

impl Store {
    fn new() -> Self {
        Self {
            jobs: HashMap::new(),
            queue: VecDeque::new(),
            nodes: HashMap::new(),
            earn: EarnLedger::default(),
            work_seq: 0,
            dirty: false,
        }
    }

    fn from_persisted(p: PersistedState) -> Self {
        Self {
            jobs: p.jobs,
            queue: p.queue,
            nodes: p.nodes,
            earn: p.earn,
            work_seq: p.work_seq,
            dirty: false,
        }
    }

    fn to_persisted(&self) -> PersistedState {
        PersistedState {
            jobs: self.jobs.clone(),
            queue: self.queue.clone(),
            nodes: self.nodes.clone(),
            earn: self.earn.clone(),
            work_seq: self.work_seq,
        }
    }
}

#[derive(Clone, Debug)]
pub struct CoordOptions {
    pub bind: String,
    pub data_dir: PathBuf,
    /// Continuously enqueue verifiable blake3_work jobs.
    pub auto_work: bool,
}

impl Default for CoordOptions {
    fn default() -> Self {
        Self {
            bind: "0.0.0.0:8787".into(),
            data_dir: crate::config::NodeConfig::default_dir().join("coord"),
            auto_work: true,
        }
    }
}

fn state_path(dir: &Path) -> PathBuf {
    dir.join("state.json")
}

fn load_store(dir: &Path) -> Store {
    let path = state_path(dir);
    match std::fs::read_to_string(&path) {
        Ok(raw) => match serde_json::from_str::<PersistedState>(&raw) {
            Ok(p) => {
                tracing::info!("loaded coordinator state from {}", path.display());
                Store::from_persisted(p)
            }
            Err(e) => {
                tracing::warn!("corrupt state ({e}) — starting empty");
                Store::new()
            }
        },
        Err(_) => Store::new(),
    }
}

pub async fn run_coordinator(bind: &str) -> Result<()> {
    run_coordinator_with(CoordOptions {
        bind: bind.into(),
        ..CoordOptions::default()
    })
    .await
}

pub async fn run_coordinator_with(opts: CoordOptions) -> Result<()> {
    std::fs::create_dir_all(&opts.data_dir)?;
    let store = load_store(&opts.data_dir);
    let app = App {
        inner: Arc::new(Mutex::new(store)),
        data_dir: opts.data_dir.clone(),
    };

    // Background: persist dirty state + optional auto-work feeder
    {
        let app_bg = app.clone();
        let auto = opts.auto_work;
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(Duration::from_secs(2));
            loop {
                tick.tick().await;
                if auto {
                    feed_auto_work(&app_bg);
                }
                flush_if_dirty(&app_bg);
            }
        });
    }

    let router = Router::new()
        .route("/health", get(health))
        .route("/v1/jobs", post(create_job))
        .route("/v1/jobs/:id", get(get_job))
        .route("/v1/jobs/complete", post(complete_job))
        .route("/v1/nodes/heartbeat", post(heartbeat))
        .route("/v1/nodes/claim", post(claim))
        .route("/v1/stats", get(stats))
        .route("/v1/earn", get(earn_stats))
        .layer(CorsLayer::permissive())
        .with_state(app);

    let addr: SocketAddr = opts.bind.parse().context("bind address")?;
    banner_coord(&addr, &opts);

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, router).await?;
    Ok(())
}

fn banner_coord(addr: &SocketAddr, opts: &CoordOptions) {
    println!("GRID coordinator (pilot fabric)");
    println!("  listen     http://{addr}");
    println!("  data       {}", opts.data_dir.display());
    println!("  auto-work  {}", if opts.auto_work { "on · blake3_work" } else { "off" });
    println!("  TSL        {}", TransactSecurityLayer::default().describe());
    println!("  routes     POST /v1/jobs · heartbeat · claim · complete · GET /v1/stats /v1/earn");
}

fn feed_auto_work(app: &App) {
    let mut g = app.inner.lock();
    let queued = g
        .queue
        .iter()
        .filter(|id| g.jobs.get(*id).map(|j| j.status == "queued").unwrap_or(false))
        .count();
    if queued >= AUTO_WORK_TARGET_QUEUED {
        return;
    }
    let need = AUTO_WORK_TARGET_QUEUED - queued;
    let now = Utc::now().timestamp() as u64;
    for _ in 0..need {
        g.work_seq = g.work_seq.saturating_add(1);
        let payload = fabric_work_payload(now, g.work_seq, AUTO_WORK_ITERS);
        let job = Job {
            id: format!("por_{}", &Uuid::new_v4().to_string()[..10]),
            kind: JobKind::Blake3Work.as_str().into(),
            payload,
            created_at: Utc::now().to_rfc3339(),
            timeout_sec: 180,
            status: "queued".into(),
            assigned_node_id: None,
            earn_credits: None,
            result_commitment: None,
            operator_pubkey: None,
        };
        g.queue.push_back(job.id.clone());
        g.jobs.insert(job.id.clone(), job);
        g.dirty = true;
    }
}

fn flush_if_dirty(app: &App) {
    let snapshot = {
        let mut g = app.inner.lock();
        if !g.dirty {
            return;
        }
        g.dirty = false;
        g.to_persisted()
    };
    // write outside lock
    let dir = &app.data_dir;
    if let Err(e) = (|| -> Result<()> {
        std::fs::create_dir_all(dir)?;
        let path = state_path(dir);
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, serde_json::to_string_pretty(&snapshot)?)?;
        std::fs::rename(&tmp, &path)?;
        snapshot.earn.save(&dir.join("earn.json"))?;
        if let Some(parent) = dir.parent() {
            let _ = snapshot.earn.save(&EarnLedger::path_in(parent));
        }
        Ok(())
    })() {
        tracing::warn!("persist failed: {e}");
        // mark dirty again
        app.inner.lock().dirty = true;
    }
}

async fn health() -> impl IntoResponse {
    Json(serde_json::json!({
        "ok": true,
        "service": "grid-coordinator",
        "phase": 1,
        "mode": "pilot-fabric",
        "tsl": "bitcoin",
        "work": "blake3_work",
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
    let kind_s = body.kind.unwrap_or_else(|| JobKind::Blake3Work.as_str().into());
    JobKind::parse(&kind_s).map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;
    let payload = body.payload.unwrap_or_else(|| {
        fabric_work_payload(Utc::now().timestamp() as u64, 0, DEFAULT_BLAKE3_ITERS)
    });
    // Validate payload early for blake3
    if let Ok(JobKind::Blake3Work) = JobKind::parse(&kind_s) {
        crate::executor::parse_blake3_payload(&payload)
            .map_err(|e| (StatusCode::BAD_REQUEST, e))?;
    }
    let job = Job {
        id: format!("job_{}", &Uuid::new_v4().to_string()[..8]),
        kind: kind_s,
        payload,
        created_at: Utc::now().to_rfc3339(),
        timeout_sec: body.timeout_sec.unwrap_or(180),
        status: "queued".into(),
        assigned_node_id: None,
        earn_credits: None,
        result_commitment: None,
        operator_pubkey: None,
    };
    let mut g = app.inner.lock();
    g.queue.push_back(job.id.clone());
    g.jobs.insert(job.id.clone(), job.clone());
    g.dirty = true;
    Ok((StatusCode::CREATED, Json(job)))
}

async fn get_job(
    State(app): State<App>,
    AxumPath(id): AxumPath<String>,
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
    label: Option<String>,
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
        gpu_model: body.gpu_model.unwrap_or_else(|| "cpu".into()),
        max_concurrent: body.max_concurrent.unwrap_or(1),
        cluster_id: body
            .cluster_id
            .unwrap_or_else(|| body.node_id.clone()),
        last_seen: Utc::now().timestamp_millis(),
        jobs_done: existing.as_ref().map(|e| e.jobs_done).unwrap_or(0),
        jobs_failed: existing.as_ref().map(|e| e.jobs_failed).unwrap_or(0),
        earn_total: existing.as_ref().map(|e| e.earn_total).unwrap_or(0.0),
        label: body
            .label
            .or_else(|| existing.map(|e| e.label))
            .unwrap_or_default(),
    };
    g.nodes.insert(body.node_id, rec.clone());
    g.dirty = true;
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
                job.assigned_node_id = Some(body.node_id.clone());
                let job_out = job.clone();
                g.dirty = true;
                return (
                    StatusCode::OK,
                    Json(serde_json::json!({ "job": job_out })),
                )
                    .into_response();
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
    #[serde(default)]
    operator_pubkey: Option<String>,
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
    let expect = expected_output(kind, &payload).map_err(|e| (StatusCode::BAD_REQUEST, e))?;
    let verified = body.ok && body.output == expect;
    let commit = result_commitment(
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
        g.earn.credit_job(
            &body.node_id,
            &body.job_id,
            earn,
            &commit,
            Utc::now().to_rfc3339(),
        );
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
            job.result_commitment = Some(commit.clone());
        }
        if let Some(pk) = body.operator_pubkey.clone() {
            job.operator_pubkey = Some(pk);
        }
        job.clone()
    };

    g.dirty = true;

    Ok(Json(serde_json::json!({
        "job": job_out,
        "verified": verified,
        "earnCredits": earn,
        "commitment": commit,
        "tsl": "bitcoin",
    })))
}

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

    let (prop_pool, inc_pool) = split_emission(POR_EVENT_MINT);
    let prop = allocate_proportional(&scores, prop_pool);
    let inc = allocate_inclusion(&scores, inc_pool);

    let mut pay = 0.0;
    if let Some((_, p)) = prop.iter().find(|(id, _)| id == winner) {
        pay += p;
    }
    if let Some((_, p)) = inc.iter().find(|(id, _)| id == winner) {
        pay += p;
    }
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
                "commitment": j.result_commitment,
            })
        })
        .collect();
    let queued = g
        .queue
        .iter()
        .filter(|id| g.jobs.get(*id).map(|j| j.status == "queued").unwrap_or(false))
        .count();
    let verified = g.jobs.values().filter(|j| j.status == "verified").count();
    let nodes: Vec<_> = g.nodes.values().cloned().collect();
    Json(serde_json::json!({
        "phase": 1,
        "mode": "pilot-fabric",
        "tsl": "bitcoin",
        "work": "blake3_work",
        "queueDepth": queued,
        "verifiedJobs": verified,
        "totalJobs": g.jobs.len(),
        "totalMinted": g.earn.total_minted,
        "jobs": jobs,
        "nodes": nodes,
    }))
}

async fn earn_stats(State(app): State<App>) -> impl IntoResponse {
    let g = app.inner.lock();
    Json(serde_json::json!({
        "totalMinted": g.earn.total_minted,
        "balances": g.earn.balances,
        "recent": g.earn.events.iter().rev().take(25).collect::<Vec<_>>(),
        "tsl": "bitcoin",
        "note": "Off-chain pilot ledger. Genesis Earn on-rail later. Bitcoin = Transact Security Layer.",
    }))
}
