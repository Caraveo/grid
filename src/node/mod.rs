//! Host + mine loops.
//!
//! * **host** — pull useful `container_work`, serve in isolated containers, higher earn  
//! * **mine** — pull PoR / transactional-security work, slower earn  
//! * **node** — both tracks (one-box operator)

use anyhow::Result;
use tracing::{debug, info, warn};

use crate::compute;
use crate::config::NodeConfig;
use crate::coord::CoordinatorClient;
use crate::earn::EarnLedger;
use crate::executor::execute;
use crate::mesh_ping;
use crate::protocol::{result_commitment, JobKind, JobTrack};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OperatorMode {
    /// Useful compute only.
    Host,
    /// Security / PoR only.
    Mine,
    /// Both loops interleaved.
    Both,
}

pub async fn run_node(cfg: NodeConfig) -> Result<()> {
    run_operator(cfg, OperatorMode::Both, None).await
}

pub async fn run_host(cfg: NodeConfig, compute_filter: Option<String>) -> Result<()> {
    run_operator(cfg, OperatorMode::Host, compute_filter).await
}

pub async fn run_mine(cfg: NodeConfig) -> Result<()> {
    run_operator(cfg, OperatorMode::Mine, None).await
}

/// **Ember** — host + mine for a named compute/realm (registry announce is done by `grid ember --start`).
pub async fn run_ember(cfg: NodeConfig, compute_name: Option<String>) -> Result<()> {
    run_operator(cfg, OperatorMode::Both, compute_name).await
}

async fn run_operator(
    cfg: NodeConfig,
    mode: OperatorMode,
    compute_filter: Option<String>,
) -> Result<()> {
    let client = CoordinatorClient::new(&cfg.coordinator);
    let config_dir = std::env::var_os("GRID_CONFIG_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(NodeConfig::default_dir);

    let operator_pubkey = std::fs::read_to_string(config_dir.join("keys").join("operator.pub"))
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());

    let mode_label = match mode {
        OperatorMode::Host => "HOST (useful compute · higher earn)",
        OperatorMode::Mine => "MINE (PoR / transactional security · slower earn)",
        OperatorMode::Both => "NODE (host + mine)",
    };

    println!("GRID {mode_label}");
    println!("  name         {}", cfg.name);
    println!("  node         {}", cfg.node_id);
    println!("  class        {}", cfg.class);
    println!("  coordinator  {}", cfg.coordinator);
    println!("  cluster      {}", cfg.cluster());
    if let Some(ref pk) = operator_pubkey {
        let short = if pk.len() > 16 { &pk[..16] } else { pk };
        println!("  operator     {short}…");
    }
    println!("  registry     {}", mesh_ping::registry_url());
    if mesh_ping::resolve_coords(&cfg).is_some() {
        println!("  globe        on");
    } else {
        println!("  globe        off");
    }

    let computes = compute::list_computes(&config_dir).unwrap_or_default();
    if mode != OperatorMode::Mine {
        if computes.is_empty() {
            println!("  computes     (none — grid launch <name> to register capacity)");
        } else {
            println!(
                "  computes     {}",
                computes
                    .iter()
                    .map(|c| format!("{}[{}]", c.name, c.visibility.as_str()))
                    .collect::<Vec<_>>()
                    .join(", ")
            );
        }
    }
    match mode {
        OperatorMode::Host => println!("  pull         container_work (isolated)"),
        OperatorMode::Mine => println!("  pull         blake3_work (security PoR)"),
        OperatorMode::Both => println!("  pull         host + mine tracks"),
    }
    println!("  (Ctrl+C to stop)\n");

    match client.health().await {
        Ok(true) => info!("coordinator healthy"),
        Ok(false) => warn!("coordinator unhealthy"),
        Err(e) => warn!("coordinator unreachable ({e}) — will retry"),
    }

    {
        let cfg_ping = cfg.clone();
        tokio::spawn(async move {
            mesh_ping::ping_globe(&cfg_ping, true).await;
        });
    }

    {
        let client = CoordinatorClient::new(&cfg.coordinator);
        let id = cfg.node_id.clone();
        let class = cfg.class.to_string();
        let gpu = cfg.gpu_model.clone();
        let max_c = cfg.max_concurrent;
        let cluster = cfg.cluster().to_string();
        let label = cfg.name.clone();
        let cfg_hb = cfg.clone();
        let cdir = config_dir.clone();
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(std::time::Duration::from_secs(15));
            loop {
                tick.tick().await;
                match client
                    .heartbeat(&id, &class, &gpu, max_c, &cluster, Some(&label))
                    .await
                {
                    Ok(_) => {
                        let _ = compute::heartbeat_computes(&cdir);
                        compute::announce_computes(&cdir, &id, &label).await;
                        mesh_ping::ping_globe(&cfg_hb, false).await;
                    }
                    Err(e) => debug!("heartbeat: {e}"),
                }
            }
        });
    }

    client
        .heartbeat(
            &cfg.node_id,
            cfg.class.as_str(),
            &cfg.gpu_model,
            cfg.max_concurrent,
            cfg.cluster(),
            Some(&cfg.name),
        )
        .await
        .ok();

    let track = match mode {
        OperatorMode::Host => "host",
        OperatorMode::Mine => "mine",
        OperatorMode::Both => "both",
    };

    loop {
        match client.claim_track(&cfg.node_id, track).await {
            Ok(Some(job)) => {
                println!("claimed {} kind={}", job.id, job.kind);
                let kind = match JobKind::parse(&job.kind) {
                    Ok(k) => k,
                    Err(e) => {
                        eprintln!("skip: {e}");
                        continue;
                    }
                };

                // Mode safety: host loop ignores mine jobs if any slip through
                if mode == OperatorMode::Host && kind.track() != JobTrack::Host {
                    eprintln!("  skip non-host job {}", job.kind);
                    continue;
                }
                if mode == OperatorMode::Mine && kind.track() != JobTrack::Mine {
                    eprintln!("  skip non-mine job {}", job.kind);
                    continue;
                }

                if let Some(ref filter) = compute_filter {
                    if kind == JobKind::ContainerWork {
                        if let Ok(spec) = compute::ContainerJobSpec::parse(&job.payload) {
                            if let Some(ref cn) = spec.compute {
                                if cn != filter {
                                    eprintln!("  skip compute={cn} (filter={filter})");
                                    continue;
                                }
                            }
                        }
                    }
                }

                let result = match kind {
                    JobKind::ContainerWork => {
                        compute::serve_container_job(&config_dir, &job.payload).await
                    }
                    _ => execute(kind, &job.payload),
                };

                if let Some(ref err) = result.error {
                    eprintln!("  exec error: {err}");
                }
                let track_tag = match kind.track() {
                    JobTrack::Host => "host",
                    JobTrack::Mine => "mine",
                };
                let commit = result_commitment(
                    &job.id,
                    &cfg.node_id,
                    result.ok,
                    &result.output,
                    result.duration_ms,
                );
                match client
                    .complete(
                        &job.id,
                        &cfg.node_id,
                        result.ok,
                        &result.output,
                        result.duration_ms,
                        operator_pubkey.as_deref(),
                    )
                    .await
                {
                    Ok((verified, earn)) => {
                        println!(
                            "finished {} track={track_tag} verified={} earn={:.4} ms={} commit={}",
                            job.id,
                            verified,
                            earn,
                            result.duration_ms,
                            &commit[..16.min(commit.len())]
                        );
                        if verified && earn > 0.0 {
                            mirror_earn(&config_dir, &cfg.node_id, &job.id, earn, &commit);
                        }
                    }
                    Err(e) => eprintln!("complete error: {e}"),
                }
            }
            Ok(None) => {
                tokio::time::sleep(std::time::Duration::from_millis(cfg.poll_ms)).await;
            }
            Err(e) => {
                debug!("claim: {e}");
                tokio::time::sleep(std::time::Duration::from_millis(cfg.poll_ms.max(2000))).await;
            }
        }
    }
}

fn mirror_earn(config_dir: &Path, node_id: &str, job_id: &str, earn: f64, commit: &str) {
    // Mint is ON-CHAIN (unclaimed lot). Burn is chain protocol — not node logic.
    use crate::chain::ChainState;
    let mut chain = ChainState::load(config_dir).unwrap_or_default();
    let actual = chain.mint_unclaimed(node_id, job_id, earn, commit);
    if actual > 0.0 {
        let _ = chain.save(config_dir);
        // mirror thin earn.json for older tools
        let path = EarnLedger::path_in(config_dir);
        let mut ledger = EarnLedger::load(&path).unwrap_or_default();
        ledger.credit_job(
            node_id,
            job_id,
            actual,
            commit,
            chrono::Utc::now().to_rfc3339(),
        );
        let _ = ledger.save(&path);
    } else if earn > 0.0 {
        tracing::warn!(
            "on-chain mint blocked (10B cap or duplicate job) — claim/exit so protocol burns free headroom job={job_id}"
        );
    }
}
