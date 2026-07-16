//! Miner loop: heartbeat → claim → execute → complete.

use anyhow::Result;
use tracing::{debug, info, warn};

use crate::config::NodeConfig;
use crate::coord::CoordinatorClient;
use crate::executor::execute;
use crate::protocol::{result_commitment, JobKind};

pub async fn run_node(cfg: NodeConfig) -> Result<()> {
    let client = CoordinatorClient::new(&cfg.coordinator);

    println!("GRID node {}", cfg.node_id);
    println!("  name         {}", cfg.name);
    println!("  class        {}", cfg.class);
    println!("  coordinator  {}", cfg.coordinator);
    println!("  gpu          {}", cfg.gpu_model);
    println!("  cluster      {}", cfg.cluster());
    println!("  (Ctrl+C to stop)\n");

    match client.health().await {
        Ok(true) => info!("coordinator healthy"),
        Ok(false) => warn!("coordinator unhealthy"),
        Err(e) => warn!("coordinator unreachable ({e}) — will retry"),
    }

    // Heartbeat task
    {
        let client = CoordinatorClient::new(&cfg.coordinator);
        let id = cfg.node_id.clone();
        let class = cfg.class.to_string();
        let gpu = cfg.gpu_model.clone();
        let max_c = cfg.max_concurrent;
        let cluster = cfg.cluster().to_string();
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(std::time::Duration::from_secs(15));
            loop {
                tick.tick().await;
                if let Err(e) = client
                    .heartbeat(&id, &class, &gpu, max_c, &cluster)
                    .await
                {
                    debug!("heartbeat: {e}");
                }
            }
        });
    }

    // Initial heartbeat (required before claim)
    client
        .heartbeat(
            &cfg.node_id,
            cfg.class.as_str(),
            &cfg.gpu_model,
            cfg.max_concurrent,
            cfg.cluster(),
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
                let _c = result_commitment(
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
                    )
                    .await
                {
                    Ok((verified, earn)) => {
                        println!("finished {} verified={} earn={:.4}", job.id, verified, earn);
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
