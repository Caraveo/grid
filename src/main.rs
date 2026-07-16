//! `grid` — Phase 1 useful mining CLI.
//!
//! ```text
//! grid coord          # start coordinator
//! grid node           # mine
//! grid submit --wait  # buy/demo job
//! ```

use anyhow::Result;
use clap::{Parser, Subcommand};
use std::path::PathBuf;

use std::time::Duration;

use grid::banner;
use grid::bench;
use grid::config::{NodeClass, NodeConfig};
use grid::coord::{run_coordinator, CoordinatorClient};
use grid::earn::EarnLedger;
use grid::node::run_node;
use grid::p2p::{run_peer, PeerOptions};
use grid::resources;
use grid::tsl::TransactSecurityLayer;

#[derive(Parser)]
#[command(name = "grid")]
#[command(about = "GRID Phase 1 — useful mining (Bitcoin = Transact Security Layer)")]
#[command(after_help = banner::BANNER)]
#[command(author, version)]
struct Cli {
    #[arg(long, global = true, env = "GRID_CONFIG_DIR")]
    config_dir: Option<PathBuf>,

    #[arg(long, global = true, default_value = "info", env = "GRID_LOG")]
    log_level: String,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Initialize ~/.grid/config.toml
    Init {
        #[arg(long)]
        name: String,
        #[arg(long, default_value = "S")]
        class: String,
        #[arg(long, default_value = "http://127.0.0.1:8787", env = "GRID_COORDINATOR")]
        coordinator: String,
    },

    /// Run the job coordinator (Phase 1 embedded server)
    Coord {
        #[arg(long, default_value = "127.0.0.1:8787")]
        bind: String,
    },

    /// Run a miner node (claim jobs, earn)
    Node {
        #[arg(long, env = "GRID_COORDINATOR")]
        coordinator: Option<String>,
        #[arg(long, env = "GRID_NODE_ID")]
        id: Option<String>,
        #[arg(long, env = "GRID_NODE_CLASS")]
        class: Option<String>,
        #[arg(long, env = "GRID_GPU_MODEL")]
        gpu: Option<String>,
        #[arg(long)]
        poll_ms: Option<u64>,
    },

    /// Alias for `grid node`
    Start {
        #[arg(long, env = "GRID_COORDINATOR")]
        coordinator: Option<String>,
    },

    /// Submit a job
    Submit {
        #[arg(long, default_value = "echo")]
        job: String,
        #[arg(long, default_value = "hello-grid")]
        payload: String,
        #[arg(long, env = "GRID_COORDINATOR", default_value = "http://127.0.0.1:8787")]
        coordinator: String,
        #[arg(long)]
        wait: bool,
    },

    /// Coordinator stats
    Stats {
        #[arg(long, env = "GRID_COORDINATOR", default_value = "http://127.0.0.1:8787")]
        coordinator: String,
    },

    /// Local config + host resources
    Status,

    /// Host resource sample
    Resources,

    /// Benchmark this machine (CPU hash + memory)
    Bench {
        /// Seconds to run the hash stress (default 3)
        #[arg(long, default_value = "3")]
        duration: u64,
        /// Emit JSON instead of human text
        #[arg(long)]
        json: bool,
    },

    /// Join the minimal TCP P2P mesh (hello, ping RTT, peer gossip)
    Peer {
        /// Listen address host:port
        #[arg(long, default_value = "127.0.0.1:9900")]
        listen: String,
        /// Dial these peers (repeatable)
        #[arg(long = "connect")]
        connect: Vec<String>,
        /// Optional: run bench first and advertise score in hello
        #[arg(long)]
        with_bench: bool,
        #[arg(long, env = "GRID_NODE_ID")]
        id: Option<String>,
        #[arg(long)]
        name: Option<String>,
        #[arg(long, default_value = "S")]
        class: String,
        /// Genesis truth URL — ban list source of truth
        #[arg(long, env = "GRID_GENESIS")]
        genesis: Option<String>,
        /// Genesis public key hex (trust anchor)
        #[arg(long, env = "GRID_GENESIS_PUBKEY")]
        genesis_pubkey: Option<String>,
    },

    /// Genesis authority (Phase 0): peer registry + signed truth
    Genesis {
        #[command(subcommand)]
        action: GenesisCmd,
    },

    /// Protect operator keys (default: passkey). See `grid auth --help`
    Auth {
        #[command(subcommand)]
        action: Option<AuthCmd>,
    },

    /// Wallet stub + Bitcoin TSL reminder
    Wallet,
}

#[derive(Subcommand)]
enum AuthCmd {
    /// Passkey encryption (iCloud / device) — same as bare `grid auth`
    Passkey,
    /// Password encryption
    Password,
    /// 24-word keyphrase encryption
    Keyphrase,
    /// password → passkey → keyphrase
    Combo,
    /// password + passkey + 24-word + master key (master DESTROYED on this node)
    Master,
    /// Plain keys on disk only (0600) — no encryption
    Nocrypt,
    /// Unlock vault for this session
    Login,
    /// Show encryption / session status
    Status,
    /// Authenticate then remove passkey/vault protection
    Delete {
        #[arg(long)]
        wipe_keys: bool,
    },
}

#[derive(Subcommand)]
enum GenesisCmd {
    /// Create genesis Ed25519 keypair (secret never leaves this host)
    Init,
    /// Serve signed truth over HTTP (read-only; no remote ban)
    Serve {
        #[arg(long, default_value = "127.0.0.1:9100")]
        bind: String,
    },
    /// Track a peer (local secret key required)
    Track {
        #[arg(long)]
        id: String,
        #[arg(long)]
        name: String,
        #[arg(long)]
        listen: String,
        #[arg(long, default_value = "S")]
        class: String,
    },
    /// Stop tracking a peer
    Untrack {
        #[arg(long)]
        id: String,
    },
    // Policy mutations (passkey/session gated). Phase 2 → consensus.
    // Hidden from casual help noise; operators who need them use them.
    #[command(hide = true)]
    Ban {
        #[arg(long)]
        id: String,
        #[arg(long)]
        reason: String,
    },
    #[command(hide = true)]
    Unban {
        #[arg(long)]
        id: String,
    },
    /// List tracked peers (+ policy set)
    List,
    /// Print genesis public key hex
    Pubkey,
    /// Fetch & verify remote truth (for operators)
    Truth {
        #[arg(long, env = "GRID_GENESIS", default_value = "http://127.0.0.1:9100")]
        url: String,
        #[arg(long, env = "GRID_GENESIS_PUBKEY")]
        pubkey: Option<String>,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    // Load operator env early (before clap reads env= attrs for subcommands that re-parse).
    // Safe: never overwrites vars already set in the shell.
    let early_config = std::env::var_os("GRID_CONFIG_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(NodeConfig::default_dir);
    load_operator_env(&early_config);

    let cli = Cli::parse();

    let filter = match cli.log_level.to_lowercase().as_str() {
        "trace" | "debug" | "info" | "warn" | "error" => cli.log_level.to_lowercase(),
        _ => "info".into(),
    };
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| filter.into()),
        )
        .with_target(false)
        .init();

    let config_dir = cli.config_dir.unwrap_or_else(NodeConfig::default_dir);
    // Re-load in case --config-dir pointed elsewhere (still no overwrite of shell vars).
    load_operator_env(&config_dir);

    match cli.command {
        Commands::Init {
            name,
            class,
            coordinator,
        } => {
            let class = NodeClass::parse(&class)?;
            let (cfg, path) = NodeConfig::init(&config_dir, name, class, coordinator)?;
            println!("✓ Node initialized");
            println!("  Name:     {}", cfg.name);
            println!("  Node ID:  {}", cfg.node_id);
            println!("  Class:    {} (S=home · M=rack · L=datacenter)", cfg.class);
            println!("  Coord:    {}", cfg.coordinator);
            println!("  Config:   {}", path.display());
            println!("\nNext:");
            println!("  grid coord          # terminal 1");
            println!("  grid node           # terminal 2");
            println!("  grid submit --wait  # terminal 3");
        }

        Commands::Coord { bind } => {
            banner::print_banner();
            println!();
            run_coordinator(&bind).await?;
        }

        Commands::Node {
            coordinator,
            id,
            class,
            gpu,
            poll_ms,
        } => {
            banner::print_mark();
            println!();
            let cfg = load_cfg(&config_dir, coordinator, id, class, gpu, poll_ms)?;
            run_node(cfg).await?;
        }

        Commands::Start { coordinator } => {
            banner::print_mark();
            println!();
            let cfg = load_cfg(&config_dir, coordinator, None, None, None, None)?;
            run_node(cfg).await?;
        }

        Commands::Submit {
            job,
            payload,
            coordinator,
            wait,
        } => {
            let client = CoordinatorClient::new(&coordinator);
            let created = client.submit(&job, &payload).await?;
            println!("{}", serde_json::to_string_pretty(&created)?);
            if wait {
                for _ in 0..30 {
                    tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                    let j = client.get_job(&created.id).await?;
                    println!("status={}", j.status);
                    if matches!(j.status.as_str(), "verified" | "rejected" | "failed") {
                        println!("{}", serde_json::to_string_pretty(&j)?);
                        break;
                    }
                }
            }
        }

        Commands::Stats { coordinator } => {
            let client = CoordinatorClient::new(&coordinator);
            let s = client.stats().await?;
            println!("{}", serde_json::to_string_pretty(&s)?);
        }

        Commands::Status => {
            banner::print_banner();
            println!();
            println!("GRID v{}  ·  Phase 1", env!("CARGO_PKG_VERSION"));
            println!("{}", TransactSecurityLayer::default().describe());
            let path = NodeConfig::path_in(&config_dir);
            if path.exists() {
                let c = NodeConfig::load(&path)?;
                println!("Node:   {} ({})", c.name, c.node_id);
                println!("Class:  {}", c.class);
                println!("Coord:  {}", c.coordinator);
            } else {
                println!("Node:   not initialized — grid init --name my-node");
            }
            let ledger = EarnLedger::load(&EarnLedger::path_in(&config_dir)).unwrap_or_default();
            if !ledger.balances.is_empty() {
                println!("Earn:   (local) {:?}", ledger.balances);
            }
            println!();
            resources::print_summary()?;
        }

        Commands::Resources => resources::print_summary()?,

        Commands::Bench { duration, json } => {
            banner::print_mark();
            println!();
            println!("Running benchmark ({duration}s)…");
            let report = bench::run(Duration::from_secs(duration.max(1)))?;
            if json {
                println!("{}", serde_json::to_string_pretty(&report)?);
            } else {
                bench::print_report(&report);
            }
        }

        Commands::Peer {
            listen,
            connect,
            with_bench,
            id,
            name,
            class,
            genesis,
            genesis_pubkey,
        } => {
            banner::print_mark();
            println!();
            let path = NodeConfig::path_in(&config_dir);
            let (node_id, node_name) = if path.exists() {
                let c = NodeConfig::load(&path)?;
                (id.unwrap_or(c.node_id), name.unwrap_or(c.name))
            } else {
                (
                    id.unwrap_or_else(|| {
                        format!("node_{}", &uuid::Uuid::new_v4().to_string()[..8])
                    }),
                    name.unwrap_or_else(|| "peer".into()),
                )
            };
            let score = if with_bench {
                println!("Quick bench for hello score…");
                let r = bench::run(Duration::from_secs(2))?;
                println!("  score={:.1}\n", r.score);
                r.score
            } else {
                0.0
            };
            let opts = PeerOptions {
                node_id,
                name: node_name,
                class,
                listen,
                connect,
                score,
                genesis_url: genesis,
                genesis_pubkey,
            };
            run_peer(opts).await?;
        }

        Commands::Genesis { action } => {
            run_genesis(&config_dir, action).await?;
        }

        Commands::Auth { action } => {
            run_auth(&config_dir, action).await?;
        }

        Commands::Wallet => {
            let tsl = TransactSecurityLayer::default();
            println!("Wallet (Phase 1 — earn on coordinator; on-rail later)");
            println!("  {}", tsl.describe());
            println!("  Exit:  GRID → BTC (hard settlement)");
            let path = NodeConfig::path_in(&config_dir);
            if path.exists() {
                let c = NodeConfig::load(&path)?;
                println!("  Node:  {}", c.node_id);
            }
        }
    }

    Ok(())
}

async fn run_auth(config_dir: &PathBuf, action: Option<AuthCmd>) -> Result<()> {
    use grid::passkey::{auth_delete, auth_init, auth_login, auth_status, AuthMode};

    let mode_or_action = action.unwrap_or(AuthCmd::Passkey);
    match mode_or_action {
        AuthCmd::Passkey => auth_init(config_dir, AuthMode::Passkey).await?,
        AuthCmd::Password => auth_init(config_dir, AuthMode::Password).await?,
        AuthCmd::Keyphrase => auth_init(config_dir, AuthMode::Keyphrase).await?,
        AuthCmd::Combo => auth_init(config_dir, AuthMode::Combo).await?,
        AuthCmd::Master => auth_init(config_dir, AuthMode::Master).await?,
        AuthCmd::Nocrypt => auth_init(config_dir, AuthMode::Nocrypt).await?,
        AuthCmd::Login => auth_login(config_dir).await?,
        AuthCmd::Status => {
            let s = auth_status(config_dir);
            println!("GRID auth status");
            println!("  mode:       {}", s.mode);
            println!("  encrypted:  {}", s.keys_encrypted);
            println!("  unlocked:   {}", s.session_unlocked);
            println!("  passkey:    {}", s.passkey_registered);
            println!("  master:     destroyed_on_node={}", s.master_destroyed);
            if let Some(pk) = s.public_key_hex {
                println!("  public:     {pk}");
            }
            println!("  {}", s.detail);
        }
        AuthCmd::Delete { wipe_keys } => auth_delete(config_dir, wipe_keys).await?,
    }
    Ok(())
}

async fn run_genesis(config_dir: &PathBuf, action: GenesisCmd) -> Result<()> {
    use grid::genesis::{
        export_pubkey_hex, generate_keypair, run_genesis_server, store::fetch_truth, GenesisStore,
    };

    match action {
        GenesisCmd::Init => {
            banner::print_banner();
            println!();
            let keys = generate_keypair(config_dir)?;
            println!("✓ Genesis keypair created");
            println!("  public:  {}", keys.public_hex());
            println!(
                "  secret:  {} (mode 0600 — never share)",
                config_dir.join("genesis/secret.key").display()
            );
            println!();
            println!("Next:");
            println!("  grid auth                 # protect operator keys");
            println!("  grid genesis serve --bind 0.0.0.0:9100");
            println!("  grid genesis track --id <peer> --name <n> --listen host:port");
            println!();
            println!("Distribute ONLY the public key to peers:");
            println!("  export GRID_GENESIS_PUBKEY={}", keys.public_hex());
        }
        GenesisCmd::Serve { bind } => {
            banner::print_banner();
            println!();
            run_genesis_server(config_dir.clone(), &bind).await?;
        }
        GenesisCmd::Track {
            id,
            name,
            listen,
            class,
        } => {
            let mut store = GenesisStore::open(config_dir)?;
            store.track(&id, &name, &listen, &class)?;
            println!("✓ tracked {id} epoch={}", store.epoch());
        }
        GenesisCmd::Untrack { id } => {
            let mut store = GenesisStore::open(config_dir)?;
            if store.untrack(&id)? {
                println!("✓ untracked {id} epoch={}", store.epoch());
            } else {
                println!("peer {id} was not tracked");
            }
        }
        GenesisCmd::Ban { id, reason } => {
            // Requires unlocked vault / passkey session when auth is initialized.
            let _ = grid::passkey::require_unlocked(config_dir, "policy mutation").await;
            let mut store = GenesisStore::open(config_dir)?;
            let id = grid::passkey::normalize_peer_target(&id);
            let rec = store.ban(&id, &reason)?;
            println!("✓ policy applied {}", rec.peer_id);
            println!("  epoch   {}", store.epoch());
        }
        GenesisCmd::Unban { id } => {
            let _ = grid::passkey::require_unlocked(config_dir, "policy mutation").await;
            let mut store = GenesisStore::open(config_dir)?;
            let id = grid::passkey::normalize_peer_target(&id);
            if store.unban(&id)? {
                println!("✓ policy cleared {id} epoch={}", store.epoch());
            } else {
                println!("no policy entry for {id}");
            }
        }
        GenesisCmd::List => {
            let store = GenesisStore::open(config_dir)?;
            println!("Genesis truth epoch={}", store.epoch());
            println!("pubkey {}", store.keys().public_hex());
            println!("\nTracked ({}):", store.list_tracked().len());
            for p in store.list_tracked() {
                println!(
                    "  · {} name={} class={} listen={}",
                    p.peer_id, p.name, p.class, p.listen
                );
            }
            // Policy set (obfuscated label in CLI output)
            let n = store.list_banned().len();
            if n > 0 {
                println!("\nPolicy set ({n} entries)");
            }
        }
        GenesisCmd::Pubkey => {
            println!("{}", export_pubkey_hex(config_dir)?);
        }
        GenesisCmd::Truth { url, pubkey } => {
            let t = fetch_truth(&url, pubkey.as_deref()).await?;
            println!("✓ signature valid");
            println!("epoch={} issued={}", t.body.epoch, t.body.issued_at);
            println!("genesis_pubkey={}", t.body.genesis_pubkey);
            println!("tracked={} banned={}", t.body.tracked.len(), t.body.banned.len());
            for b in &t.body.banned {
                println!("  BAN {} — {}", b.peer_id, b.reason);
            }
        }
    }
    Ok(())
}

/// Load `config_dir/env` (and `~/.grid/env` fallback) into process env.
/// Does not override variables already set in the shell. Never prints secrets.
fn load_operator_env(config_dir: &std::path::Path) {
    let candidates = [
        config_dir.join("env"),
        NodeConfig::default_dir().join("env"),
    ];
    let mut seen = std::collections::HashSet::new();
    for path in candidates {
        if !seen.insert(path.clone()) {
            continue;
        }
        let Ok(raw) = std::fs::read_to_string(&path) else {
            continue;
        };
        for line in raw.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let Some((k, v)) = line.split_once('=') else {
                continue;
            };
            let k = k.trim();
            if k.is_empty() || std::env::var_os(k).is_some() {
                continue;
            }
            let v = v.trim().trim_matches(|c| c == '"' || c == '\'');
            // SAFETY: single-threaded at startup before workers; sets operator config only.
            unsafe { std::env::set_var(k, v) };
        }
    }
}

fn load_cfg(
    dir: &PathBuf,
    coordinator: Option<String>,
    id: Option<String>,
    class: Option<String>,
    gpu: Option<String>,
    poll_ms: Option<u64>,
) -> Result<NodeConfig> {
    let path = NodeConfig::path_in(dir);
    let mut cfg = if path.exists() {
        NodeConfig::load(&path)?
    } else {
        let class = NodeClass::parse(class.as_deref().unwrap_or("S"))?;
        let (c, p) = NodeConfig::init(
            dir,
            "ephemeral",
            class,
            coordinator
                .clone()
                .unwrap_or_else(|| "http://127.0.0.1:8787".into()),
        )?;
        println!("(wrote {})", p.display());
        c
    };

    if let Some(c) = coordinator {
        cfg.coordinator = c;
    }
    if let Some(i) = id {
        cfg.node_id = i;
    }
    if let Some(c) = class {
        cfg.class = NodeClass::parse(&c)?;
    }
    if let Some(g) = gpu {
        cfg.gpu_model = g;
    }
    if let Some(p) = poll_ms {
        cfg.poll_ms = p;
    }
    Ok(cfg)
}
