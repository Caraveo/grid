//! `grid` — Phase 1 useful mining CLI.
//!
//! ```text
//! grid launch garage --public   # name a compute
//! grid claim fire               # claim realm (passkey + operator key)
//! grid host                     # pull useful work · higher earn
//! grid mine                     # PoR security work · slower earn
//! grid coord                    # coordinator
//! ```

use anyhow::Result;
use clap::{Parser, Subcommand};
use std::path::PathBuf;

use std::time::Duration;

use grid::banner;
use grid::bench;
use grid::compute::{self, ComputeVisibility, DEFAULT_IMAGE};
use grid::config::{NodeClass, NodeConfig};
use grid::coord::{run_coordinator_with, CoordOptions, CoordinatorClient};
use grid::earn::EarnLedger;
use grid::node::{run_host, run_mine, run_node};
use grid::p2p::{run_peer, PeerOptions};
use grid::resources;
use grid::tsl::TransactSecurityLayer;

#[derive(Parser)]
#[command(name = "grid")]
#[command(
    about = "GRID — host useful compute · mine security PoR · Bitcoin TSL"
)]
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

    /// Run the persistent pilot coordinator (auto blake3_work by default)
    Coord {
        /// Bind address (use 0.0.0.0:8787 to accept LAN peers)
        #[arg(long, default_value = "0.0.0.0:8787", env = "GRID_COORD_BIND")]
        bind: String,
        /// State directory (jobs, nodes, earn)
        #[arg(long, env = "GRID_COORD_DATA")]
        data_dir: Option<PathBuf>,
        /// Disable continuous PoR job feeder
        #[arg(long)]
        no_auto_work: bool,
    },

    /// Launch a named compute you host exclusively on the grid
    Launch {
        /// Compute name (e.g. garage, render-1)
        name: String,
        /// Allowlisted container image
        #[arg(long, default_value = DEFAULT_IMAGE)]
        image: String,
        /// Public tunnel/announce (default)
        #[arg(long, group = "vis")]
        public: bool,
        /// Fabric-only — no public endpoint
        #[arg(long, group = "vis")]
        private: bool,
        /// Runtime backend
        #[arg(long, default_value = "docker")]
        backend: String,
        #[arg(long, default_value = "1.0")]
        cpus: f64,
        #[arg(long, default_value = "512")]
        memory: u64,
        #[arg(long, default_value = "1")]
        replicas: u32,
        #[arg(long, default_value = "S")]
        class: String,
        /// Optional service port for public hint
        #[arg(long)]
        port: Option<u16>,
    },

    /// HOST — pull useful container work, serve isolated, higher earn
    Host {
        #[arg(long, env = "GRID_COORDINATOR")]
        coordinator: Option<String>,
        /// Only serve jobs for this compute name
        #[arg(long)]
        compute: Option<String>,
        #[arg(long, env = "GRID_NODE_ID")]
        id: Option<String>,
        #[arg(long)]
        poll_ms: Option<u64>,
    },

    /// MINE — PoR / transactional security work, slower earn
    Mine {
        #[arg(long, env = "GRID_COORDINATOR")]
        coordinator: Option<String>,
        #[arg(long, env = "GRID_NODE_ID")]
        id: Option<String>,
        #[arg(long)]
        poll_ms: Option<u64>,
    },

    /// Manage named computes
    Compute {
        #[command(subcommand)]
        action: ComputeCmd,
    },

    /// Host + mine together (one-box)
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

    /// Alias for `grid node` (host + mine)
    Start {
        #[arg(long, env = "GRID_COORDINATOR")]
        coordinator: Option<String>,
    },

    /// Submit a job (default: blake3_work mine PoR)
    Submit {
        #[arg(long, default_value = "blake3_work")]
        job: String,
        /// blake3_work: seed|iters · container_work: JSON or image|cmd…
        #[arg(long, default_value = "")]
        payload: String,
        #[arg(long, env = "GRID_COORDINATOR", default_value = "http://127.0.0.1:8787")]
        coordinator: String,
        #[arg(long)]
        wait: bool,
    },

    /// Coordinator stats + earn
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

    /// Earn balances + Bitcoin TSL exit reminder
    Wallet {
        #[arg(long, env = "GRID_COORDINATOR", default_value = "http://127.0.0.1:8787")]
        coordinator: String,
    },

    /// Public mesh registry (default: https://grid-compute.com)
    Registry {
        /// Override registry base URL
        #[arg(long, env = "GRID_REGISTRY_URL")]
        url: Option<String>,
        /// Emit JSON
        #[arg(long)]
        json: bool,
    },

    /// Claim a mesh realm (`grid://name.grid`) with IdentityKey + operator Ed25519
    Claim {
        /// Realm to claim: fire | fire.grid | grid://fire.grid
        /// Use `list` to show local claims.
        realm: String,
        /// Show claim status only (no new claim ceremony)
        #[arg(long)]
        status: bool,
        /// Emit JSON
        #[arg(long)]
        json: bool,
    },

    /// Ember = host + mine + compute + registry for a realm (e.g. fire.grid)
    Ember {
        /// Realm / compute name: fire | fire.grid | grid://fire.grid
        name: String,
        /// Announce to registry + start host+mine for this compute
        #[arg(long)]
        start: bool,
        /// Emit JSON status
        #[arg(long)]
        json: bool,
    },

    /// Paid registry activation — Cash App $5 → $Caraveo (exact note). Donations accepted.
    Register {
        /// Realm name to activate on registry.grid
        name: String,
        /// Human label
        #[arg(long)]
        label: Option<String>,
        /// After Cash App payment, submit for review
        #[arg(long)]
        confirm: bool,
        /// Optional Cash App confirmation text
        #[arg(long)]
        cash_confirm: Option<String>,
        /// Status only (no new registration)
        #[arg(long)]
        status: bool,
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
enum ComputeCmd {
    /// List computes on this machine
    List,
    /// Status for one compute
    Status {
        name: String,
    },
    /// Query public compute registry for available capacity (grid-compute.com)
    Available {
        /// Include busy/offline, not only available
        #[arg(long)]
        all: bool,
        #[arg(long)]
        json: bool,
        #[arg(long, env = "GRID_REGISTRY_URL")]
        url: Option<String>,
    },
    /// Re-announce local computes to the public registry
    Announce,
    /// Stop capacity (keep manifest)
    Stop {
        name: String,
    },
    /// Start / re-ready from manifest
    Start {
        name: String,
    },
    /// Stop and delete local state
    Destroy {
        name: String,
    },
    /// Container logs (if any runtime ids)
    Logs {
        name: String,
        #[arg(short, long)]
        follow: bool,
    },
    /// Export portable manifest JSON (change machines)
    Export {
        name: String,
    },
    /// Import manifest JSON from file or stdin (-)
    Import {
        path: String,
    },
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
            println!("  grid coord");
            println!("  grid launch garage --public");
            println!("  grid host            # useful work · higher earn");
            println!("  grid mine            # security PoR · slower earn");
        }

        Commands::Coord {
            bind,
            data_dir,
            no_auto_work,
        } => {
            banner::print_banner();
            println!();
            let data_dir = data_dir.unwrap_or_else(|| config_dir.join("coord"));
            let opts = CoordOptions {
                bind,
                data_dir,
                auto_work: !no_auto_work,
            };
            run_coordinator_with(opts).await?;
        }

        Commands::Launch {
            name,
            image,
            public,
            private,
            backend,
            cpus,
            memory,
            replicas,
            class,
            port,
        } => {
            banner::print_mark();
            println!();
            // default public unless --private
            let visibility = if private && !public {
                ComputeVisibility::Private
            } else {
                ComputeVisibility::Public
            };
            let m = compute::launch(
                &config_dir,
                &name,
                &image,
                visibility,
                &backend,
                cpus,
                memory,
                replicas,
                &class,
                port,
            )
            .await?;
            println!("✓ Compute launched: {}", m.name);
            println!("  image       {}", m.image);
            println!("  visibility  {}", m.visibility.as_str());
            println!("  backend     {}", m.backend);
            println!("  replicas    {}", m.replicas);
            println!("  class       {}", m.class);
            println!("  machine     {}", m.machine_id);
            if let Some(u) = &m.public_url {
                println!("  endpoint    {u}");
            }
            println!("\nNext:");
            println!("  grid host              # pull & serve useful jobs (higher earn)");
            println!("  grid compute list");
            println!("  grid mine              # optional: security PoR (slower earn)");
        }

        Commands::Host {
            coordinator,
            compute,
            id,
            poll_ms,
        } => {
            banner::print_mark();
            println!();
            let cfg = load_cfg(&config_dir, coordinator, id, None, None, poll_ms)?;
            std::env::set_var("GRID_CONFIG_DIR", &config_dir);
            run_host(cfg, compute).await?;
        }

        Commands::Mine {
            coordinator,
            id,
            poll_ms,
        } => {
            banner::print_mark();
            println!();
            let cfg = load_cfg(&config_dir, coordinator, id, None, None, poll_ms)?;
            std::env::set_var("GRID_CONFIG_DIR", &config_dir);
            run_mine(cfg).await?;
        }

        Commands::Compute { action } => match action {
            ComputeCmd::List => compute::print_list(&config_dir)?,
            ComputeCmd::Status { name } => compute::print_status(&config_dir, &name)?,
            ComputeCmd::Available { all, json, url } => {
                let snap = compute::fetch_computes(url.as_deref(), !all).await?;
                compute::print_computes(&snap, json)?;
            }
            ComputeCmd::Announce => {
                let path = NodeConfig::path_in(&config_dir);
                let (node_id, label) = if path.exists() {
                    let c = NodeConfig::load(&path)?;
                    (c.node_id, c.name)
                } else {
                    ("node_local".into(), "local".into())
                };
                compute::announce_computes(&config_dir, &node_id, &label).await;
                println!("announced local computes → {}", grid::mesh_ping::registry_url());
                println!("  check: grid compute available");
            }
            ComputeCmd::Stop { name } => {
                compute::stop(&config_dir, &name)?;
                println!("stopped {name}");
            }
            ComputeCmd::Start { name } => {
                let m = compute::load_manifest(&config_dir, &name)?;
                let m = compute::launch(
                    &config_dir,
                    &m.name,
                    &m.image,
                    m.visibility,
                    &m.backend,
                    m.cpus,
                    m.memory_mb,
                    m.replicas,
                    &m.class,
                    m.port,
                )
                .await?;
                println!("ready {} ({})", m.name, m.visibility.as_str());
            }
            ComputeCmd::Destroy { name } => {
                compute::destroy(&config_dir, &name)?;
                println!("destroyed {name}");
            }
            ComputeCmd::Logs { name, follow } => compute::logs(&config_dir, &name, follow)?,
            ComputeCmd::Export { name } => {
                println!("{}", compute::export_compute(&config_dir, &name)?);
            }
            ComputeCmd::Import { path } => {
                let raw = if path == "-" {
                    use std::io::Read;
                    let mut s = String::new();
                    std::io::stdin().read_to_string(&mut s)?;
                    s
                } else {
                    std::fs::read_to_string(&path)?
                };
                let m = compute::import_compute(&config_dir, &raw)?;
                println!(
                    "imported {} — run: grid compute start {}",
                    m.name, m.name
                );
            }
        },

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
            std::env::set_var("GRID_CONFIG_DIR", &config_dir);
            run_node(cfg).await?;
        }

        Commands::Start { coordinator } => {
            banner::print_mark();
            println!();
            let cfg = load_cfg(&config_dir, coordinator, None, None, None, None)?;
            std::env::set_var("GRID_CONFIG_DIR", &config_dir);
            run_node(cfg).await?;
        }

        Commands::Submit {
            job,
            payload,
            coordinator,
            wait,
        } => {
            let client = CoordinatorClient::new(&coordinator);
            let payload = if payload.is_empty() && job == "blake3_work" {
                format!(
                    "submit:{}:{}|{}",
                    chrono::Utc::now().timestamp(),
                    &uuid::Uuid::new_v4().to_string()[..8],
                    grid::executor::DEFAULT_BLAKE3_ITERS
                )
            } else if payload.is_empty() && (job == "container_work" || job == "container" || job == "host") {
                serde_json::json!({
                    "image": "alpine:3.20",
                    "cmd": ["echo", format!("host-{}", &uuid::Uuid::new_v4().to_string()[..8])],
                    "timeoutSec": 60
                })
                .to_string()
            } else if payload.is_empty() {
                "grid".into()
            } else {
                payload
            };
            let created = client.submit(&job, &payload).await?;
            println!("{}", serde_json::to_string_pretty(&created)?);
            if wait {
                for _ in 0..120 {
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
            println!("Registry: {}", grid::mesh_ping::registry_url());
            println!("Tracks:   host = useful containers (higher earn)");
            println!("          mine = PoR security (slower earn)");
            let path = NodeConfig::path_in(&config_dir);
            if path.exists() {
                let c = NodeConfig::load(&path)?;
                println!("Node:   {} ({})", c.name, c.node_id);
                println!("Class:  {}", c.class);
                println!("Coord:  {}", c.coordinator);
            } else {
                println!("Node:   not initialized — grid init --name my-node");
            }
            let computes = compute::list_computes(&config_dir).unwrap_or_default();
            if computes.is_empty() {
                println!("Computes: (none) — grid launch <name>");
            } else {
                println!(
                    "Computes: {}",
                    computes
                        .iter()
                        .map(|c| c.name.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                );
            }
            let ledger = EarnLedger::load(&EarnLedger::path_in(&config_dir)).unwrap_or_default();
            if !ledger.balances.is_empty() {
                println!("Earn:   (local) {:?}", ledger.balances);
            }
            println!();
            resources::print_summary()?;
        }

        Commands::Registry { url, json } => {
            let snap = grid::mesh_ping::fetch_registry(url.as_deref()).await?;
            grid::mesh_ping::print_registry(&snap, json)?;
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

        Commands::Wallet { coordinator } => {
            let tsl = TransactSecurityLayer::default();
            println!("GRID wallet · pilot earn ledger");
            println!("  {}", tsl.describe());
            println!("  Exit path: GRID credits → BTC (Transact Security Layer)");
            let path = NodeConfig::path_in(&config_dir);
            let node_id = if path.exists() {
                let c = NodeConfig::load(&path)?;
                println!("  Node:   {} ({})", c.name, c.node_id);
                Some(c.node_id)
            } else {
                None
            };
            let local = EarnLedger::load(&EarnLedger::path_in(&config_dir)).unwrap_or_default();
            if let Some(ref id) = node_id {
                println!("  Local:  {:.6} credits", local.balance(id));
            }
            println!("  Minted: {:.6} (local mirror)", local.total_minted);
            let client = CoordinatorClient::new(&coordinator);
            match client.stats().await {
                Ok(s) => {
                    if let Some(tm) = s.get("totalMinted").and_then(|v| v.as_f64()) {
                        println!("  Coord:  totalMinted={tm:.6}");
                    }
                    if let Some(nodes) = s.get("nodes").and_then(|v| v.as_array()) {
                        for n in nodes {
                            let id = n.get("nodeId").and_then(|v| v.as_str()).unwrap_or("?");
                            let earn = n.get("earnTotal").and_then(|v| v.as_f64()).unwrap_or(0.0);
                            let done = n.get("jobsDone").and_then(|v| v.as_u64()).unwrap_or(0);
                            println!("  · {id}  earn={earn:.4}  jobs_done={done}");
                        }
                    }
                }
                Err(e) => println!("  Coord:  offline ({e})"),
            }
            println!("\n  On-rail Genesis Earn + BTC exit ships when emission is public.");
            println!("  Until then this ledger is real accounting for verified PoR work.");
        }

        Commands::Claim { realm, status, json } => {
            banner::print_mark();
            println!();
            let realm_l = realm.trim().to_lowercase();
            if realm_l == "list" || realm_l == "ls" {
                if json {
                    let items = grid::claim::list_claims(&config_dir)?;
                    println!("{}", serde_json::to_string_pretty(&items)?);
                } else {
                    grid::claim::print_list(&config_dir)?;
                }
            } else if status || realm_l == "status" {
                // `grid claim status fire` not supported as two args; use --status
                let name = if realm_l == "status" {
                    anyhow::bail!("usage: grid claim <realm> --status");
                } else {
                    realm
                };
                let claim = grid::claim::load_claim(&config_dir, &name)?;
                if json {
                    println!("{}", serde_json::to_string_pretty(&claim)?);
                } else {
                    grid::claim::print_claim(&claim);
                }
            } else {
                // Availability hint (non-fatal)
                match grid::claim::check_registry_name(
                    &grid::claim::normalize_realm(&realm).unwrap_or_else(|_| realm.clone()),
                )
                .await
                {
                    Ok(v) => {
                        if let Some(avail) = v.get("available").and_then(|x| x.as_bool()) {
                            if avail {
                                println!(
                                    "registry: name available for registration on registry.grid"
                                );
                            } else {
                                let reason = v
                                    .get("reason")
                                    .and_then(|x| x.as_str())
                                    .unwrap_or("taken");
                                println!(
                                    "registry: name already on registry.grid ({reason}) — binding local identity"
                                );
                            }
                        }
                    }
                    Err(_) => {}
                }
                let claim = grid::claim::claim_realm(&config_dir, &realm).await?;
                if json {
                    println!("{}", serde_json::to_string_pretty(&claim)?);
                } else {
                    println!();
                    println!("✓ Claimed {}", claim.realm);
                    grid::claim::print_claim(&claim);
                    println!();
                    println!("Next:");
                    println!("  grid ember {}            # host + mine + compute + registry", claim.name);
                    println!("  grid ember {} --start", claim.name);
                    println!("  open grid://{}.grid", claim.name);
                }
            }
        }

        Commands::Ember { name, start, json } => {
            banner::print_mark();
            println!();
            if start {
                let cfg = load_cfg(&config_dir, None, None, None, None, None)?;
                grid::ember::start_ember(&config_dir, &name, cfg).await?;
            } else {
                let st = grid::ember::status(&config_dir, &name).await?;
                if json {
                    println!("{}", serde_json::to_string_pretty(&st)?);
                } else {
                    grid::ember::print_status(&st);
                }
            }
        }

        Commands::Register {
            name,
            label,
            confirm,
            cash_confirm,
            status,
            json,
        } => {
            banner::print_mark();
            println!();
            if status {
                let a = grid::register::fetch_activation(&name).await?;
                if let Some(local) = grid::register::load_local(&config_dir, &name) {
                    if json {
                        println!(
                            "{}",
                            serde_json::to_string_pretty(&serde_json::json!({
                                "activation": a,
                                "local": local,
                            }))?
                        );
                    } else {
                        grid::register::print_activation(&a);
                        println!();
                        println!("Local registration");
                        println!("  id     {}", local.reg_id);
                        println!("  status {}", local.status);
                        println!("  note   {}", local.payment_note);
                        if !a.activated {
                            grid::register::print_pay_instructions(&local);
                        }
                    }
                } else if json {
                    println!("{}", serde_json::to_string_pretty(&a)?);
                } else {
                    grid::register::print_activation(&a);
                    if !a.activated {
                        println!();
                        println!("  Start:  grid register {name}");
                    }
                }
            } else if confirm {
                let rec = grid::register::confirm_payment(
                    &config_dir,
                    &name,
                    cash_confirm.as_deref(),
                )
                .await?;
                if json {
                    println!("{}", serde_json::to_string_pretty(&rec)?);
                } else {
                    println!("✓ Payment claimed → pending review");
                    println!("  name    {}", rec.name);
                    println!("  id      {}", rec.reg_id);
                    println!("  status  {}", rec.status);
                    println!("  note    {}", rec.payment_note);
                    println!();
                    println!("  Admin verifies Cash App at {} then activates.", rec.cashtag);
                    println!("  When active: grid ember {} · grid compute announce", rec.name);
                    println!("  Donations accepted at {} anytime.", rec.cashtag);
                }
            } else {
                // show activation first if already active
                let a = grid::register::fetch_activation(&name).await?;
                if a.activated {
                    if json {
                        println!("{}", serde_json::to_string_pretty(&a)?);
                    } else {
                        grid::register::print_activation(&a);
                        println!();
                        println!("  Already active — grid ember {name}");
                    }
                } else if let Some(local) =
                    grid::register::load_local(&config_dir, &name)
                {
                    if json {
                        println!("{}", serde_json::to_string_pretty(&local)?);
                    } else {
                        println!("Registration already started for {}", local.name);
                        grid::register::print_pay_instructions(&local);
                    }
                } else {
                    let rec = grid::register::start_registration(
                        &config_dir,
                        &name,
                        label.as_deref(),
                    )
                    .await?;
                    if json {
                        println!("{}", serde_json::to_string_pretty(&rec)?);
                    } else {
                        println!("✓ Registration started for grid://{}.grid", rec.name);
                        println!("  id  {}", rec.reg_id);
                        grid::register::print_pay_instructions(&rec);
                    }
                }
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
