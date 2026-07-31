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

use crate::blockchain::Settlement;
use crate::chain::{ChainState, MAX_SUPPLY};
use crate::earn::EarnLedger;
use crate::executor::{expected_output, fabric_work_payload, DEFAULT_BLAKE3_ITERS};
use crate::por::{
    allocate_inclusion, allocate_proportional, effective_score, inputs_from_jobs, split_emission,
    NodeScore,
};
use crate::protocol::{job_intent_commitment, result_commitment, Job, JobKind, JobTrack, NodeInfo};
use crate::tsl::TransactSecurityLayer;

/// Credits per verified **mine** (PoR / transactional security) event — slower earn.
const POR_EVENT_MINT: f64 = 100.0;
/// Credits per verified **host** (useful container) event — higher earn.
const HOST_EVENT_MINT: f64 = 400.0;
/// Keep the mine queue fed so miners always have security work.
const AUTO_WORK_TARGET_QUEUED: usize = 8;
const AUTO_WORK_ITERS: u64 = DEFAULT_BLAKE3_ITERS;
/// Optional host demo jobs (allowlisted echo) when auto_work on.
const AUTO_HOST_TARGET_QUEUED: usize = 2;

/// Fail closed: a private pilot can verify jobs without accidentally creating
/// value. Public issuance requires explicit operator activation after replica
/// validation/audit work is complete.
fn earnings_enabled() -> bool {
    matches!(
        std::env::var("GRID_ENABLE_EARN").ok().as_deref(),
        Some("1") | Some("true") | Some("TRUE")
    )
}

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
    #[serde(default)]
    launchers: HashMap<String, LauncherTrust>,
    earn: EarnLedger,
    #[serde(default)]
    settlements: Vec<Settlement>,
    work_seq: u64,
}

struct Store {
    jobs: HashMap<String, Job>,
    queue: VecDeque<String>,
    nodes: HashMap<String, NodeInfo>,
    launchers: HashMap<String, LauncherTrust>,
    earn: EarnLedger,
    settlements: Vec<Settlement>,
    work_seq: u64,
    dirty: bool,
}

impl Store {
    fn new() -> Self {
        Self {
            jobs: HashMap::new(),
            queue: VecDeque::new(),
            nodes: HashMap::new(),
            launchers: HashMap::new(),
            earn: EarnLedger::default(),
            settlements: vec![],
            work_seq: 0,
            dirty: false,
        }
    }

    fn from_persisted(p: PersistedState) -> Self {
        Self {
            jobs: p.jobs,
            queue: p.queue,
            nodes: p.nodes,
            launchers: p.launchers,
            earn: p.earn,
            settlements: p.settlements,
            work_seq: p.work_seq,
            dirty: false,
        }
    }

    fn to_persisted(&self) -> PersistedState {
        PersistedState {
            jobs: self.jobs.clone(),
            queue: self.queue.clone(),
            nodes: self.nodes.clone(),
            launchers: self.launchers.clone(),
            earn: self.earn.clone(),
            settlements: self.settlements.clone(),
            work_seq: self.work_seq,
        }
    }
}

/// Coordinator-side admission record. A launcher must have a durable public
/// identity before it can request an interactive container.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LauncherTrust {
    pub public_key: String,
    #[serde(default = "launcher_neutral")]
    pub reputation: f64,
    #[serde(default)]
    pub rejected_requests: u64,
    #[serde(default)]
    pub completed_jobs: u64,
    #[serde(default)]
    pub banned: bool,
    #[serde(default)]
    pub ban_reason: Option<String>,
}

fn launcher_neutral() -> f64 {
    1.0
}

fn valid_launcher_key(key: &str) -> bool {
    key.len() == 64 && key.bytes().all(|b| b.is_ascii_hexdigit())
}

/// Requests containing host-escape controls are never schedulable. Keep this
/// deliberately structural so ordinary job text mentioning "root" cannot ban
/// someone; these are Docker/Kubernetes privilege mechanisms only.
fn host_escape_attempt(payload: &str) -> bool {
    let p = payload.to_ascii_lowercase();
    [
        "\"privileged\"",
        "\"hostnetwork\"",
        "\"hostpid\"",
        "/var/run/docker.sock",
        "--pid=host",
        "--net=host",
        "\"capadd\"",
        "\"hostpath\"",
    ]
    .iter()
    .any(|needle| p.contains(needle))
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
    println!(
        "  auto-work  {}",
        if opts.auto_work {
            "on · blake3_work"
        } else {
            "off"
        }
    );
    println!(
        "  TSL        {}",
        TransactSecurityLayer::default().describe()
    );
    println!("  routes     POST /v1/jobs · heartbeat · claim · complete · GET /v1/stats /v1/earn");
}

fn feed_auto_work(app: &App) {
    let mut g = app.inner.lock();
    let mine_queued = g
        .queue
        .iter()
        .filter(|id| {
            g.jobs
                .get(*id)
                .map(|j| {
                    j.status == "queued"
                        && JobKind::parse(&j.kind)
                            .map(|k| k.track() == JobTrack::Mine)
                            .unwrap_or(true)
                })
                .unwrap_or(false)
        })
        .count();
    if mine_queued < AUTO_WORK_TARGET_QUEUED {
        let need = AUTO_WORK_TARGET_QUEUED - mine_queued;
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
                intent_commitment: None,
                launcher_pubkey: None,
                status: "queued".into(),
                assigned_node_id: None,
                earn_credits: None,
                result_commitment: None,
                operator_pubkey: None,
            };
            let mut job = job;
            job.intent_commitment = Some(job_intent_commitment(
                &job.id,
                &job.kind,
                &job.payload,
                &job.created_at,
                job.timeout_sec,
            ));
            g.queue.push_back(job.id.clone());
            g.jobs.insert(job.id.clone(), job);
            g.dirty = true;
        }
    }

    // Host track: small allowlisted container echo jobs (useful-serve demo)
    let host_queued = g
        .queue
        .iter()
        .filter(|id| {
            g.jobs
                .get(*id)
                .map(|j| {
                    j.status == "queued"
                        && JobKind::parse(&j.kind)
                            .map(|k| k.track() == JobTrack::Host)
                            .unwrap_or(false)
                })
                .unwrap_or(false)
        })
        .count();
    if host_queued < AUTO_HOST_TARGET_QUEUED {
        let need = AUTO_HOST_TARGET_QUEUED - host_queued;
        for _ in 0..need {
            g.work_seq = g.work_seq.saturating_add(1);
            let token = format!("grid-host-{}", g.work_seq);
            let payload = serde_json::json!({
                "image": "alpine:3.20",
                "cmd": ["echo", token],
                "timeoutSec": 60,
                "cpus": 0.25,
                "memoryMb": 128,
                "network": false
            })
            .to_string();
            let job = Job {
                id: format!("host_{}", &Uuid::new_v4().to_string()[..10]),
                kind: JobKind::ContainerWork.as_str().into(),
                payload,
                created_at: Utc::now().to_rfc3339(),
                timeout_sec: 120,
                intent_commitment: None,
                launcher_pubkey: None,
                status: "queued".into(),
                assigned_node_id: None,
                earn_credits: None,
                result_commitment: None,
                operator_pubkey: None,
            };
            let mut job = job;
            job.intent_commitment = Some(job_intent_commitment(
                &job.id,
                &job.kind,
                &job.payload,
                &job.created_at,
                job.timeout_sec,
            ));
            g.queue.push_back(job.id.clone());
            g.jobs.insert(job.id.clone(), job);
            g.dirty = true;
        }
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
    #[serde(default)]
    launcher_pubkey: Option<String>,
}

async fn create_job(
    State(app): State<App>,
    Json(body): Json<CreateJobBody>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let kind_s = body
        .kind
        .unwrap_or_else(|| JobKind::Blake3Work.as_str().into());
    let kind = JobKind::parse(&kind_s).map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;
    let payload = body.payload.unwrap_or_else(|| {
        fabric_work_payload(Utc::now().timestamp() as u64, 0, DEFAULT_BLAKE3_ITERS)
    });
    let launcher = body.launcher_pubkey.map(|k| k.to_lowercase());
    if kind == JobKind::ContainerWork && !launcher.as_deref().is_some_and(valid_launcher_key) {
        return Err((
            StatusCode::FORBIDDEN,
            "container jobs require a 32-byte launcher public key".into(),
        ));
    }
    if let Some(ref key) = launcher {
        if !valid_launcher_key(key) {
            return Err((
                StatusCode::BAD_REQUEST,
                "launcher public key must be 32-byte hex".into(),
            ));
        }
        let mut g = app.inner.lock();
        let trust = g
            .launchers
            .entry(key.clone())
            .or_insert_with(|| LauncherTrust {
                public_key: key.clone(),
                reputation: 1.0,
                rejected_requests: 0,
                completed_jobs: 0,
                banned: false,
                ban_reason: None,
            });
        if trust.banned {
            return Err((
                StatusCode::FORBIDDEN,
                format!(
                    "launcher banned: {}",
                    trust.ban_reason.as_deref().unwrap_or("policy")
                ),
            ));
        }
        if kind == JobKind::ContainerWork && host_escape_attempt(&payload) {
            trust.rejected_requests += 1;
            trust.reputation = 0.0;
            trust.banned = true;
            trust.ban_reason =
                Some("attempted container host escape or privilege elevation".into());
            g.dirty = true;
            return Err((
                StatusCode::FORBIDDEN,
                "launcher banned: host escape controls are forbidden".into(),
            ));
        }
    }
    // Validate payload early
    if let Ok(JobKind::Blake3Work) = JobKind::parse(&kind_s) {
        crate::executor::parse_blake3_payload(&payload)
            .map_err(|e| (StatusCode::BAD_REQUEST, e))?;
    }
    if let JobKind::ContainerWork = kind {
        crate::compute::ContainerJobSpec::parse(&payload)
            .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;
    }
    let job = Job {
        id: format!("job_{}", &Uuid::new_v4().to_string()[..8]),
        kind: kind_s,
        payload,
        created_at: Utc::now().to_rfc3339(),
        timeout_sec: body.timeout_sec.unwrap_or(180),
        intent_commitment: None,
        launcher_pubkey: launcher,
        status: "queued".into(),
        assigned_node_id: None,
        earn_credits: None,
        result_commitment: None,
        operator_pubkey: None,
    };
    let mut job = job;
    job.intent_commitment = Some(job_intent_commitment(
        &job.id,
        &job.kind,
        &job.payload,
        &job.created_at,
        job.timeout_sec,
    ));
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

async fn heartbeat(State(app): State<App>, Json(body): Json<HeartbeatBody>) -> impl IntoResponse {
    let mut g = app.inner.lock();
    let existing = g.nodes.get(&body.node_id).cloned();
    let rec = NodeInfo {
        node_id: body.node_id.clone(),
        class: body.class.unwrap_or_else(|| "S".into()),
        gpu_model: body.gpu_model.unwrap_or_else(|| "cpu".into()),
        max_concurrent: body.max_concurrent.unwrap_or(1),
        cluster_id: body.cluster_id.unwrap_or_else(|| body.node_id.clone()),
        last_seen: Utc::now().timestamp_millis(),
        jobs_done: existing.as_ref().map(|e| e.jobs_done).unwrap_or(0),
        jobs_failed: existing.as_ref().map(|e| e.jobs_failed).unwrap_or(0),
        earn_total: existing.as_ref().map(|e| e.earn_total).unwrap_or(0.0),
        reputation: existing.as_ref().map(|e| e.reputation).unwrap_or(1.0),
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
    /// `host` | `mine` | `both` (default both for back-compat)
    #[serde(default)]
    track: Option<String>,
    /// Required since v0.2.24. Missing versions from older clients fail closed.
    #[serde(default)]
    cli_version: String,
}

fn track_matches(job_kind: &str, want: &str) -> bool {
    let want = want.to_lowercase();
    if want.is_empty() || want == "both" || want == "all" {
        return true;
    }
    let Ok(k) = JobKind::parse(job_kind) else {
        return want == "mine";
    };
    match want.as_str() {
        "host" => k.track() == JobTrack::Host,
        "mine" => k.track() == JobTrack::Mine,
        _ => true,
    }
}

async fn claim(State(app): State<App>, Json(body): Json<ClaimBody>) -> impl IntoResponse {
    let minimum = crate::version_gate::configured_minimum();
    if let Err(error) = crate::version_gate::require_minimum(&body.cli_version, &minimum) {
        return (
            StatusCode::UPGRADE_REQUIRED,
            Json(serde_json::json!({
                "error": error.to_string(),
                "currentVersion": body.cli_version,
                "minimumVersion": minimum,
            })),
        )
            .into_response();
    }
    let mut g = app.inner.lock();
    if !g.nodes.contains_key(&body.node_id) {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": "heartbeat first" })),
        )
            .into_response();
    }
    let want = body.track.unwrap_or_else(|| "both".into());
    let n = g.queue.len();
    for _ in 0..n {
        let Some(id) = g.queue.pop_front() else {
            break;
        };
        let take = g
            .jobs
            .get(&id)
            .map(|j| j.status == "queued" && track_matches(&j.kind, &want))
            .unwrap_or(false);
        if take {
            if let Some(job) = g.jobs.get_mut(&id) {
                job.status = "assigned".into();
                job.assigned_node_id = Some(body.node_id.clone());
                let job_out = job.clone();
                g.dirty = true;
                return (StatusCode::OK, Json(serde_json::json!({ "job": job_out })))
                    .into_response();
            }
        } else {
            // put back non-matching still-queued jobs
            if g.jobs
                .get(&id)
                .map(|j| j.status == "queued")
                .unwrap_or(false)
            {
                g.queue.push_back(id);
            }
        }
    }
    (
        StatusCode::NO_CONTENT,
        Json(serde_json::json!({ "job": null })),
    )
        .into_response()
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
    #[serde(default)]
    solana_reward_wallet: Option<String>,
}

fn queue_devnet_solana_reward(
    job_id: String,
    recipient: String,
    amount: f64,
    commitment: String,
) -> bool {
    let Ok(base) = std::env::var("GRID_SOLANA_RELAYER_URL") else {
        return false;
    };
    let Ok(secret) = std::env::var("GRID_SOLANA_RELAYER_SECRET") else {
        tracing::warn!("GRID_SOLANA_RELAYER_URL is set but GRID_SOLANA_RELAYER_SECRET is missing");
        return false;
    };
    let Ok(url) = url::Url::parse(&base) else {
        tracing::warn!("invalid GRID_SOLANA_RELAYER_URL");
        return false;
    };
    if !matches!(url.host_str(), Some("127.0.0.1" | "localhost" | "::1")) {
        tracing::warn!("refusing non-local GRID_SOLANA_RELAYER_URL");
        return false;
    }
    let Ok(endpoint) = url.join("/v1/rewards") else {
        tracing::warn!("invalid GRID Solana reward endpoint");
        return false;
    };

    tokio::spawn(async move {
        let result = reqwest::Client::new()
            .post(endpoint)
            .bearer_auth(secret)
            .json(&serde_json::json!({
                "jobId": job_id,
                "recipient": recipient,
                "amount": format!("{amount:.9}"),
                "commitment": commitment,
            }))
            .send()
            .await;
        match result {
            Ok(response) if response.status().is_success() => {
                match response.json::<serde_json::Value>().await {
                    Ok(body) => tracing::info!(
                        "Solana devnet GRID minted job={} signature={} explorer={}",
                        job_id,
                        body.get("signature")
                            .and_then(|v| v.as_str())
                            .unwrap_or("?"),
                        body.get("explorer").and_then(|v| v.as_str()).unwrap_or("?"),
                    ),
                    Err(error) => tracing::warn!("Solana reward response decode failed: {error}"),
                }
            }
            Ok(response) => {
                let status = response.status();
                let detail = response.text().await.unwrap_or_default();
                tracing::warn!("Solana devnet reward rejected ({status}): {detail}");
            }
            Err(error) => tracing::warn!("Solana devnet reward relayer unavailable: {error}"),
        }
    });
    true
}

async fn complete_job(
    State(app): State<App>,
    Json(body): Json<CompleteBody>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let mut g = app.inner.lock();

    let (kind_s, payload, assigned, intent) = {
        let job = g
            .jobs
            .get(&body.job_id)
            .ok_or((StatusCode::BAD_REQUEST, "unknown job".into()))?;
        (
            job.kind.clone(),
            job.payload.clone(),
            job.assigned_node_id.clone(),
            job.intent_commitment.clone().unwrap_or_else(|| {
                job_intent_commitment(
                    &job.id,
                    &job.kind,
                    &job.payload,
                    &job.created_at,
                    job.timeout_sec,
                )
            }),
        )
    };

    if let Some(ref a) = assigned {
        if a != &body.node_id {
            return Err((StatusCode::FORBIDDEN, "not assignee".into()));
        }
    }

    let kind = JobKind::parse(&kind_s).map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;
    let verified = if kind == JobKind::ContainerWork {
        // Host path: predict echo-style output or accept ok + non-empty for allowlisted images
        match expected_output(kind, &payload) {
            Ok(expect) => body.ok && body.output.trim() == expect.trim(),
            Err(_) => {
                // Fallback: ok + non-empty output (async docker re-run optional later)
                body.ok && !body.output.trim().is_empty()
            }
        }
    } else {
        let expect = expected_output(kind, &payload).map_err(|e| (StatusCode::BAD_REQUEST, e))?;
        body.ok && body.output == expect
    };
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
            node.reputation = (node.reputation + 0.02).min(1.5);
        } else {
            node.jobs_failed += 1;
            node.reputation = (node.reputation - 0.15).max(0.5);
        }
    }
    if let Some(key) = g
        .jobs
        .get(&body.job_id)
        .and_then(|j| j.launcher_pubkey.clone())
    {
        if let Some(launcher) = g.launchers.get_mut(&key) {
            if verified {
                launcher.completed_jobs += 1;
                launcher.reputation = (launcher.reputation + 0.01).min(1.5);
            } else {
                launcher.reputation = (launcher.reputation - 0.05).max(0.25);
            }
        }
    }

    let mut earn = 0.0;
    let mut settlement = None;
    if verified {
        let pool = if kind.track() == JobTrack::Host {
            HOST_EVENT_MINT
        } else {
            POR_EVENT_MINT
        };
        let (raw, record) = credit_event(
            &g,
            &body.node_id,
            pool,
            &body.job_id,
            kind.track(),
            &intent,
            &commit,
        );
        settlement = Some(record);
        if earnings_enabled() {
            // On-chain mint (unclaimed). Protocol burns live on the chain, not node/wallet.
            let root = app.data_dir.parent().unwrap_or(app.data_dir.as_path());
            let mut chain = ChainState::load(root).unwrap_or_default();
            earn = chain.mint_unclaimed(&body.node_id, &body.job_id, raw, &commit);
            let _ = chain.save(root);
            if earn > 0.0 {
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
    if let Some(record) = settlement {
        g.settlements.push(record);
    }

    g.dirty = true;
    let solana_reward_queued = if earn > 0.0 {
        body.solana_reward_wallet
            .clone()
            .map(|wallet| {
                queue_devnet_solana_reward(body.job_id.clone(), wallet, earn, commit.clone())
            })
            .unwrap_or(false)
    } else {
        false
    };

    Ok(Json(serde_json::json!({
        "job": job_out,
        "verified": verified,
        "earnCredits": earn,
        "commitment": commit,
        "tsl": "bitcoin",
        "earningsEnabled": earnings_enabled(),
        "solanaRewardQueued": solana_reward_queued,
    })))
}

fn credit_event(
    store: &Store,
    winner: &str,
    event_mint: f64,
    job_id: &str,
    track: JobTrack,
    intent: &str,
    result: &str,
) -> (f64, Settlement) {
    let scores: Vec<NodeScore> = store
        .nodes
        .values()
        .map(|n| {
            let online = Utc::now().timestamp_millis() - n.last_seen < 60_000;
            let mut inputs = inputs_from_jobs(n.jobs_done, n.jobs_failed, online);
            inputs.reputation = n.reputation;
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

    let (prop_pool, inc_pool) = split_emission(event_mint);
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
    let settlement = Settlement::from_scores(
        job_id.into(),
        match track {
            JobTrack::Host => "host",
            JobTrack::Mine => "mine",
        }
        .into(),
        intent.into(),
        result.into(),
        event_mint,
        &scores,
    );
    (pay, settlement)
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
        .filter(|id| {
            g.jobs
                .get(*id)
                .map(|j| j.status == "queued")
                .unwrap_or(false)
        })
        .count();
    let verified = g.jobs.values().filter(|j| j.status == "verified").count();
    let nodes: Vec<_> = g.nodes.values().cloned().collect();
    Json(serde_json::json!({
        "phase": 1,
        "mode": "pilot-fabric",
        "tsl": "bitcoin",
        "work": ["blake3_work", "container_work"],
        "tracks": { "host": "container_work (higher earn)", "mine": "blake3_work (slower earn)" },
        "queueDepth": queued,
        "verifiedJobs": verified,
        "totalJobs": g.jobs.len(),
        "totalMinted": g.earn.total_minted,
        "maxSupply": MAX_SUPPLY,
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
