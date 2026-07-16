//! Miner loop: heartbeat → claim → execute verifiable PoR → complete → earn.

use anyhow::Result;
use tracing::{debug, info, warn};

use crate::config::NodeConfig;
use crate::coord::CoordinatorClient;
use crate::earn::EarnLedger;
use crate::executor::execute;
use crate::mesh_ping;
use crate::protocol::{result_commitment, JobKind};

pub async fn run_node(cfg: NodeConfig) -> Result<()> {
    let client = CoordinatorClient::new(&cfg.coordinator);
    let config_dir = crate::config::NodeConfig::default_dir();
    // Prefer operator config dir if GRID_CONFIG_DIR set
    let config_dir = std::env::var_os("GRID_CONFIG_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or(config_dir);

    let operator_pubkey = std::fs::read_to_string(config_dir.join("keys").join("operator.pub"))
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());

    println!("GRID miner {}", cfg.node_id);
    println!("  name         {}", cfg.name);
    println!("  class        {}", cfg.class);
    println!("  coordinator  {}", cfg.coordinator);
    println!("  gpu          {}", cfg.gpu_model);
    println!("  cluster      {}", cfg.cluster());
    if let Some(ref pk) = operator_pubkey {
        let short = if pk.len() > 16 { &pk[..16] } else { pk };
        println!("  operator     {short}…");
    }
    if mesh_ping::resolve_coords(&cfg).is_some() {
        println!("  globe        opt-in (location-only ping)");
    } else {
        println!("  globe        off");
    }
    println!("  work         blake3_work (verifiable CPU PoR)");
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
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(std::time::Duration::from_secs(15));
            loop {
                tick.tick().await;
                match client
                    .heartbeat(&id, &class, &gpu, max_c, &cluster, Some(&label))
                    .await
                {
                    Ok(_) => {
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

    loop {
        match client.claim(&cfg.node_id).await {
            Ok(Some(job)) => {
                println!("claimed {} kind={}", job.id, job.kind);
                let kind = match JobKind::parse(&job.kind) {
                    Ok(k) => k,
                    Err(e) => {
                        eprintln!("skip: {e}");
                        continue;
                    }
                };
                let result = execute(kind, &job.payload);
                if let Some(ref err) = result.error {
                    eprintln!("  exec error: {err}");
                }
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
                            "finished {} verified={} earn={:.4} ms={} commit={}",
                            job.id,
                            verified,
                            earn,
                            result.duration_ms,
                            &commit[..16.min(commit.len())]
                        );
                        if verified && earn > 0.0 {
                            // Local mirror of earn (coord also persists)
                            let path = EarnLedger::path_in(&config_dir);
                            let mut ledger = EarnLedger::load(&path).unwrap_or_default();
                            ledger.credit_job(
                                &cfg.node_id,
                                &job.id,
                                earn,
                                &commit,
                                chrono::Utc::now().to_rfc3339(),
                            );
                            let _ = ledger.save(&path);
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
