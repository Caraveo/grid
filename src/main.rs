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

const DEFAULT_COORDINATOR: &str = "https://coordinator.grid-compute.com";
const DEFAULT_GENESIS_PEER: &str = grid::genesis::CANONICAL_P2P_PEER;
const DEFAULT_GENESIS_URL: &str = grid::genesis::CANONICAL_TRUTH_URL;
const DEFAULT_GENESIS_PUBKEY: &str = grid::genesis::CANONICAL_LEADER_PUBKEY;

#[derive(Parser)]
#[command(name = "grid")]
#[command(about = "GRID — host useful compute · mine security PoR · Bitcoin TSL")]
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
        #[arg(
            long,
            default_value = "https://coordinator.grid-compute.com",
            env = "GRID_COORDINATOR"
        )]
        coordinator: String,
        /// Solana wallet for verified GRID rewards (devnet pilot)
        #[arg(long, env = "GRID_SOLANA_REWARD_WALLET")]
        solana_wallet: Option<String>,
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
        #[arg(long, default_value = "containerd")]
        backend: String,
        #[arg(long, default_value = "1.0")]
        cpus: f64,
        #[arg(long, default_value = "512")]
        memory: u64,
        #[arg(long, default_value = "1")]
        replicas: u32,
        #[arg(long, default_value = "S")]
        class: String,
        /// Fixed isolated container service port (must be 41783)
        #[arg(long, default_value = "41783")]
        port: Option<u16>,
    },

    /// HOST — pull useful container work, serve isolated, higher earn
    Host {
        #[arg(long, env = "GRID_COORDINATOR", default_value = DEFAULT_COORDINATOR)]
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
        #[arg(long, env = "GRID_COORDINATOR", default_value = DEFAULT_COORDINATOR)]
        coordinator: Option<String>,
        #[arg(long, env = "GRID_NODE_ID")]
        id: Option<String>,
        #[arg(long)]
        poll_ms: Option<u64>,
        #[arg(long, default_value = "0.0.0.0:9900")]
        p2p_listen: String,
        #[arg(long = "p2p-connect")]
        p2p_connect: Vec<String>,
        #[arg(long)]
        p2p_no_genesis: bool,
        #[arg(long)]
        p2p_noise_key_file: Option<PathBuf>,
    },

    /// Manage named computes
    Compute {
        #[command(subcommand)]
        action: ComputeCmd,
    },

    /// P2P peer + host + mine together (one-box)
    Node {
        #[arg(long, env = "GRID_COORDINATOR", default_value = DEFAULT_COORDINATOR)]
        coordinator: Option<String>,
        #[arg(long, env = "GRID_NODE_ID")]
        id: Option<String>,
        #[arg(long, env = "GRID_NODE_CLASS")]
        class: Option<String>,
        #[arg(long, env = "GRID_GPU_MODEL")]
        gpu: Option<String>,
        #[arg(long)]
        poll_ms: Option<u64>,
        /// P2P listen address
        #[arg(long, default_value = "0.0.0.0:9900")]
        p2p_listen: String,
        /// Additional P2P peers to dial (repeatable; Genesis is automatic)
        #[arg(long = "p2p-connect")]
        p2p_connect: Vec<String>,
        /// Do not dial the canonical Genesis peer
        #[arg(long)]
        p2p_no_genesis: bool,
        /// Dedicated mode-0600 Noise key for a non-interactive node service
        #[arg(long)]
        p2p_noise_key_file: Option<PathBuf>,
    },

    /// Alias for `grid node` (P2P peer + host + mine)
    Start {
        /// GRID Engine YAML manifest (for example: grid start node.yaml)
        manifest: Option<PathBuf>,
        #[arg(long, env = "GRID_COORDINATOR", default_value = DEFAULT_COORDINATOR)]
        coordinator: Option<String>,
    },

    /// Platform-aware isolated runtime and P2P node scaffolding
    Engine {
        #[command(subcommand)]
        action: EngineCmd,
    },

    /// Submit a job (default: blake3_work mine PoR)
    Submit {
        #[arg(long, default_value = "blake3_work")]
        job: String,
        /// blake3_work: seed|iters · container_work: JSON or image|cmd…
        #[arg(long, default_value = "")]
        payload: String,
        #[arg(
            long,
            env = "GRID_COORDINATOR",
            default_value = "https://coordinator.grid-compute.com"
        )]
        coordinator: String,
        #[arg(long)]
        wait: bool,
    },

    /// Coordinator stats + earn
    Stats {
        #[arg(
            long,
            env = "GRID_COORDINATOR",
            default_value = "https://coordinator.grid-compute.com"
        )]
        coordinator: String,
    },

    /// Local config + host resources
    Status,

    /// Production-mainnet launch gate (never promotes a single-signer pilot)
    Mainnet {
        /// Emit machine-readable JSON
        #[arg(long)]
        json: bool,
        /// One-time, non-destructive blocks.json → immutable per-height files
        #[arg(long)]
        migrate_storage: bool,
    },

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
        #[arg(long, default_value = "0.0.0.0:9900")]
        listen: String,
        /// Dial these peers (repeatable; Genesis is included automatically)
        #[arg(long = "connect")]
        connect: Vec<String>,
        /// Do not dial the canonical Genesis peer (for the Genesis host itself)
        #[arg(long)]
        no_genesis: bool,
        /// Dedicated mode-0600 Noise key for a non-interactive P2P service
        #[arg(long)]
        noise_key_file: Option<PathBuf>,
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
        #[arg(long, env = "GRID_GENESIS", default_value = DEFAULT_GENESIS_URL)]
        genesis: Option<String>,
        /// Genesis public key hex (trust anchor)
        #[arg(
            long,
            env = "GRID_GENESIS_PUBKEY",
            default_value = DEFAULT_GENESIS_PUBKEY
        )]
        genesis_pubkey: Option<String>,
        /// Bind GP identity + announce dial directory for this realm
        #[arg(long)]
        realm: Option<String>,
        /// Skip registry dial announce (local mesh only)
        #[arg(long)]
        no_announce: bool,
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

    /// GRID wallet (`grid0…` addresses) — claim / send / receive
    Wallet {
        #[command(subcommand)]
        action: Option<WalletCmd>,
    },

    /// Solana reward wallet — create, import, and inspect devnet GRID
    Solana {
        #[command(subcommand)]
        action: Option<SolanaCmd>,
    },

    /// Local JSON bridge for the official native GRID Wallet applications
    #[command(hide = true)]
    Gui {
        #[command(subcommand)]
        action: GuiCmd,
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

    /// GRID Protocol identity helpers (internal id; UI stays grid://)
    Gp {
        #[command(subcommand)]
        action: GpCmd,
    },

    /// Consent-gated compliance collection (IP/MAC for admin forensics only)
    Compliance {
        #[command(subcommand)]
        action: ComplianceCmd,
    },
}

#[derive(Subcommand)]
enum GpCmd {
    /// Show stable 128-hex identity for a realm (derived from operator key + realm)
    Id {
        /// Realm: fire | fire.grid | grid://fire.grid
        realm: String,
        #[arg(long)]
        json: bool,
    },
    /// Resolve P2P listen targets for a realm (or 128-hex id)
    Resolve {
        query: String,
        #[arg(long)]
        json: bool,
    },
    /// TCP dial check via registry dial directory
    Dial {
        /// Realm or 128-hex id
        query: String,
    },
    /// Fetch + verify permanent Key/Verified cert from registry
    Cert {
        realm: String,
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
enum ComplianceCmd {
    /// Show consent copy and enable collection (type yes)
    Enable {
        /// Skip interactive prompt (scripts only — still records consent)
        #[arg(long)]
        yes: bool,
    },
    /// Disable collection
    Disable,
    /// Local consent status
    Status {
        #[arg(long)]
        json: bool,
    },
    /// Push attestation for a realm (requires enable)
    Push {
        realm: String,
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
enum ComputeCmd {
    /// List computes on this machine
    List,
    /// Status for one compute
    Status { name: String },
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
    Stop { name: String },
    /// Start / re-ready from manifest
    Start { name: String },
    /// Stop and delete local state
    Destroy { name: String },
    /// Container logs (if any runtime ids)
    Logs {
        name: String,
        #[arg(short, long)]
        follow: bool,
    },
    /// Export portable manifest JSON (change machines)
    Export { name: String },
    /// Import manifest JSON from file or stdin (-)
    Import { path: String },
}

#[derive(Subcommand)]
enum EngineCmd {
    /// Inspect this platform's production container-runtime prerequisites
    Doctor,
    /// Install and verify the supported isolated container runtime for this platform
    Install,
    /// Start the Engine-managed host only (no Proof-of-Resource mining)
    Start {
        #[arg(long, env = "GRID_COORDINATOR", default_value = DEFAULT_COORDINATOR)]
        coordinator: Option<String>,
        /// Only serve jobs for this named compute
        #[arg(long)]
        compute: Option<String>,
        #[arg(long, env = "GRID_NODE_ID")]
        id: Option<String>,
        #[arg(long)]
        poll_ms: Option<u64>,
    },
    /// Write a reviewable GRID Engine YAML manifest without overwriting files
    Init {
        #[arg(default_value = "grid-engine.yaml")]
        path: PathBuf,
        #[arg(long, default_value = "grid-node")]
        name: String,
    },
}

#[derive(Subcommand)]
enum WalletCmd {
    /// Create wallet from operator key → grid0 address
    Init,
    /// Show status, balances, unclaimed mint + burn warnings
    Status,
    /// Print your grid0 receive address
    Address,
    /// Move unclaimed node mint into your grid0 wallet (before 1y burn)
    Claim {
        /// Optional max amount to claim
        #[arg(long)]
        amount: Option<f64>,
    },
    /// Send GRID to another grid0 address
    Send {
        /// Destination grid0 address
        to: String,
        /// Amount of GRID
        amount: f64,
        /// Optional memo
        #[arg(long)]
        memo: Option<String>,
    },
    /// Show receive address (inbound credits land after claim or receive-tx)
    Receive,
    /// Import a signed send tx JSON file addressed to you
    ReceiveTx {
        /// Path to tx JSON (- for stdin)
        path: String,
        /// Sender operator pubkey hex (required to verify)
        #[arg(long)]
        from_pubkey: Option<String>,
    },
    /// Recent wallet transactions
    History {
        #[arg(long, default_value = "20")]
        limit: usize,
    },
    /// Report / apply **on-chain protocol burns** (blockchain rule, not wallet)
    BurnCheck {
        /// Only report, do not write burns
        #[arg(long)]
        dry_run: bool,
    },
}

#[derive(Subcommand)]
enum SolanaCmd {
    /// Create a new local Solana reward keypair (stored mode 0600)
    Create,
    /// Use an existing Solana address (watch-only; no private key imported)
    Import {
        /// Solana wallet address that should receive mined GRID
        address: String,
    },
    /// Show configured address and live devnet GRID balance
    Status,
}

#[derive(Subcommand)]
enum GuiCmd {
    /// Print the non-secret wallet state as JSON
    Snapshot,
    /// Read one secret-bearing action as JSON from stdin
    Action,
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
    /// Create signed block genesis from the configured authority
    ChainInit {
        /// Load a mode-0600 operational key on a dedicated Genesis host
        #[arg(long)]
        operational: bool,
    },
    /// Serve signed truth over HTTP (read-only; no remote ban)
    Serve {
        #[arg(long, default_value = "127.0.0.1:9100")]
        bind: String,
        /// Load a mode-0600 operational key on a dedicated Genesis host
        #[arg(long)]
        operational: bool,
    },
    /// Poll verified coordinator settlements and produce signed GRID blocks
    Produce {
        #[arg(
            long,
            env = "GRID_COORDINATOR",
            default_value = "https://coordinator.grid-compute.com"
        )]
        coordinator: String,
        #[arg(long, default_value = "5")]
        poll_secs: u64,
        #[arg(long, default_value = "25")]
        batch: usize,
        /// Load the mode-0600 operational leader on a dedicated Genesis host
        #[arg(long)]
        operational: bool,
        /// Produce at most one block, or exit successfully if the queue is empty
        #[arg(long)]
        once: bool,
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
    /// Export the leader key to a new 0600 file for a dedicated Genesis host
    #[command(hide = true)]
    ExportLeader {
        #[arg(long)]
        output: PathBuf,
    },
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
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| filter.into()),
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
            solana_wallet,
        } => {
            let class = NodeClass::parse(&class)?;
            let (mut cfg, path) = NodeConfig::init(&config_dir, name, class, coordinator)?;
            cfg.solana_reward_wallet = solana_wallet;
            cfg.save(&path)?;
            println!("✓ Node initialized");
            println!("  Name:     {}", cfg.name);
            println!("  Node ID:  {}", cfg.node_id);
            println!("  Class:    {} (S=home · M=rack · L=datacenter)", cfg.class);
            println!("  Coord:    {}", cfg.coordinator);
            println!(
                "  Solana:   {}",
                cfg.solana_reward_wallet
                    .as_deref()
                    .unwrap_or("not configured")
            );
            println!("  Config:   {}", path.display());
            println!("\nNext:");
            println!("  grid mine            # security PoR + devnet GRID rewards");
            println!("  grid launch garage --public");
            println!("  grid host            # useful work (public verifier coming later)");
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
            run_engine_host(cfg, compute).await?;
        }

        Commands::Mine {
            coordinator,
            id,
            poll_ms,
            p2p_listen,
            p2p_connect,
            p2p_no_genesis,
            p2p_noise_key_file,
        } => {
            banner::print_mark();
            println!();
            let cfg = load_cfg(&config_dir, coordinator, id, None, None, poll_ms)?;
            std::env::set_var("GRID_CONFIG_DIR", &config_dir);
            run_miner_node(
                cfg,
                config_dir.clone(),
                p2p_listen,
                p2p_connect,
                p2p_no_genesis,
                p2p_noise_key_file,
            ).await?;
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
                println!(
                    "announced local computes → {}",
                    grid::mesh_ping::registry_url()
                );
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
                println!("imported {} — run: grid compute start {}", m.name, m.name);
            }
        },

        Commands::Node {
            coordinator,
            id,
            class,
            gpu,
            poll_ms,
            p2p_listen,
            p2p_connect,
            p2p_no_genesis,
            p2p_noise_key_file,
        } => {
            banner::print_mark();
            println!();
            let cfg = load_cfg(&config_dir, coordinator, id, class, gpu, poll_ms)?;
            std::env::set_var("GRID_CONFIG_DIR", &config_dir);
            run_combined_node(
                cfg,
                config_dir.clone(),
                p2p_listen,
                p2p_connect,
                p2p_no_genesis,
                p2p_noise_key_file,
            )
            .await?;
        }

        Commands::Start { manifest, coordinator } => {
            banner::print_mark();
            println!();
            let engine = manifest.as_deref().map(grid::engine::load).transpose()?;
            let cfg = load_cfg(
                &config_dir,
                coordinator,
                None,
                engine.as_ref().map(|m| m.spec.class.clone()),
                None,
                None,
            )?;
            std::env::set_var("GRID_CONFIG_DIR", &config_dir);
            if let Some(ref m) = engine {
                println!("GRID Engine manifest: {}", m.metadata.name);
                println!("  runtime: {} CPU · {} MiB · job network={}", m.spec.runtime.cpus, m.spec.runtime.memory_mib, m.spec.runtime.network_for_jobs);
                println!("  storage: encrypted persistent volumes · {} MiB quota", m.spec.storage.volume_quota_mib);
                run_engine_peer(cfg, config_dir.clone(), m.spec.p2p.listen.clone(), m.spec.p2p.connect.clone()).await?;
                return Ok(());
            }
            run_combined_node(
                cfg,
                config_dir.clone(),
                "0.0.0.0:9900".into(),
                Vec::new(),
                false,
                None,
            )
            .await?;
        }

        Commands::Engine { action } => match action {
            EngineCmd::Doctor => grid::engine::doctor(),
            EngineCmd::Install => grid::engine::install()?,
            EngineCmd::Start { coordinator, compute, id, poll_ms } => {
                banner::print_mark();
                println!();
                let cfg = load_cfg(&config_dir, coordinator, id, None, None, poll_ms)?;
                std::env::set_var("GRID_CONFIG_DIR", &config_dir);
                run_engine_host(cfg, compute).await?;
            }
            EngineCmd::Init { path, name } => {
                grid::engine::scaffold(&path, &name)?;
                println!("✓ GRID Engine manifest written: {}", path.display());
                println!("  review it, then: grid start {}", path.display());
            }
        },

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
            } else if payload.is_empty()
                && (job == "container_work" || job == "container" || job == "host")
            {
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
            println!("Config:   {}", config_dir.display());
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

            // Blockchain size + local security audit
            grid::chain::print_status_blockchain(&config_dir);

            println!();
            resources::print_summary()?;
        }

        Commands::Mainnet {
            json,
            migrate_storage,
        } => {
            if migrate_storage {
                let blocks = grid::blockchain::ChainReplica::migrate_to_split_storage(&config_dir)?;
                if !json {
                    println!(
                        "✓ migrated {blocks} block(s) to {}",
                        grid::blockchain::ChainReplica::manifest_path_in(&config_dir).display()
                    );
                    println!("  legacy blocks.json retained as a read-only snapshot");
                    println!();
                }
            }
            let report = grid::consensus::readiness(&config_dir);
            grid::consensus::print_readiness(&report, json)?;
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
            mut connect,
            no_genesis,
            noise_key_file,
            with_bench,
            id,
            name,
            class,
            genesis,
            genesis_pubkey,
            realm,
            no_announce,
        } => {
            banner::print_mark();
            println!();
            if !no_genesis && !connect.iter().any(|peer| peer == DEFAULT_GENESIS_PEER) {
                connect.insert(0, DEFAULT_GENESIS_PEER.to_string());
            }
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
            let (gp_id, realm_n, pubkey_hex) = if let Some(ref r) = realm {
                let g = grid::gp::gp_id_for_operator(&config_dir, r).ok();
                let pk = grid::passkey::operator_pubkey_hex(&config_dir).ok();
                let rn = grid::claim::normalize_realm(r).ok();
                if !no_announce {
                    if let (Some(_), Some(ref realm_ok)) = (&g, &rn) {
                        if let Err(e) = grid::gp::p2p_announce(&config_dir, realm_ok, &listen).await
                        {
                            eprintln!("warn: dial announce failed: {e}");
                        }
                    }
                }
                (g, rn, pk)
            } else {
                (None, None, None)
            };
            let noise_static_key = if let Some(path) = noise_key_file {
                let raw = std::fs::read(&path)?;
                raw.as_slice()
                    .try_into()
                    .map_err(|_| anyhow::anyhow!("P2P Noise key must be exactly 32 bytes"))?
            } else {
                let dek =
                    grid::passkey::require_unlocked(&config_dir, "start encrypted P2P").await?;
                grid::passkey::p2p_noise_static_key(&config_dir, &dek)?
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
                gp_id,
                realm: realm_n,
                pubkey_hex,
                noise_static_key,
                config_dir: config_dir.clone(),
            };
            run_peer(opts).await?;
        }

        Commands::Genesis { action } => {
            run_genesis(&config_dir, action).await?;
        }

        Commands::Auth { action } => {
            run_auth(&config_dir, action).await?;
        }

        Commands::Wallet { action } => {
            run_wallet(&config_dir, action.unwrap_or(WalletCmd::Status)).await?;
        }

        Commands::Solana { action } => match action.unwrap_or(SolanaCmd::Status) {
            SolanaCmd::Create => {
                let address = grid::solana_wallet::create(&config_dir)?;
                println!("✓ Solana reward wallet created");
                println!("  address  {address}");
                println!(
                    "  keypair  {}",
                    config_dir.join("keys/solana-reward.json").display()
                );
                println!("  network  devnet");
                println!("  backup this keypair; GRID cannot recover it");
            }
            SolanaCmd::Import { address } => {
                let address = grid::solana_wallet::import_address(&config_dir, &address)?;
                println!("✓ Solana reward address configured (watch-only)");
                println!("  address  {address}");
            }
            SolanaCmd::Status => grid::solana_wallet::status(&config_dir).await?,
        },

        Commands::Gui { action } => match action {
            GuiCmd::Snapshot => {
                let snapshot = grid::gui::snapshot(&config_dir).await;
                println!("{}", serde_json::to_string(&snapshot)?);
            }
            GuiCmd::Action => {
                let mut raw = String::new();
                std::io::Read::read_to_string(&mut std::io::stdin(), &mut raw)?;
                let action: grid::gui::WalletAction = serde_json::from_str(&raw)?;
                let result = grid::gui::act(&config_dir, action).await?;
                println!("{}", serde_json::to_string(&result)?);
            }
        },

        Commands::Claim {
            realm,
            status,
            json,
        } => {
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
                                let reason =
                                    v.get("reason").and_then(|x| x.as_str()).unwrap_or("taken");
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
                    println!(
                        "  grid ember {}            # host + mine + compute + registry",
                        claim.name
                    );
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
                let rec =
                    grid::register::confirm_payment(&config_dir, &name, cash_confirm.as_deref())
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
                    println!(
                        "  Admin verifies Cash App at {} then activates.",
                        rec.cashtag
                    );
                    println!(
                        "  When active: grid ember {} · grid compute announce",
                        rec.name
                    );
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
                } else if let Some(local) = grid::register::load_local(&config_dir, &name) {
                    if json {
                        println!("{}", serde_json::to_string_pretty(&local)?);
                    } else {
                        println!("Registration already started for {}", local.name);
                        grid::register::print_pay_instructions(&local);
                    }
                } else {
                    let rec =
                        grid::register::start_registration(&config_dir, &name, label.as_deref())
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
        Commands::Gp { action } => match action {
            GpCmd::Id { realm, json } => {
                grid::gp::print_gp_id(&config_dir, &realm, json)?;
            }
            GpCmd::Resolve { query, json } => {
                let peers = grid::gp::resolve_dial(&query).await?;
                if json {
                    println!("{}", serde_json::to_string_pretty(&peers)?);
                } else if peers.is_empty() {
                    println!("No dial targets for «{query}»");
                } else {
                    for (gp, realm, listen) in peers {
                        println!(
                            "  grid://{realm}.grid  {listen}  id={}…",
                            &gp[..gp.len().min(12)]
                        );
                    }
                }
            }
            GpCmd::Dial { query } => {
                grid::gp::dial_once(&query).await?;
            }
            GpCmd::Cert { realm, json } => {
                let c = grid::gp::verify_remote_cert(&realm).await?;
                if json {
                    println!("{}", serde_json::to_string_pretty(&c)?);
                } else {
                    grid::gp::print_cert(&c);
                }
            }
        },
        Commands::Compliance { action } => match action {
            ComplianceCmd::Enable { yes } => {
                grid::gp::compliance_enable(&config_dir, yes)?;
            }
            ComplianceCmd::Disable => {
                grid::gp::compliance_disable(&config_dir)?;
            }
            ComplianceCmd::Status { json } => {
                let s = grid::gp::compliance_status(&config_dir);
                if json {
                    println!("{}", serde_json::to_string_pretty(&s)?);
                } else {
                    println!(
                        "compliance {}",
                        if s.enabled { "ENABLED" } else { "disabled" }
                    );
                    if s.enabled {
                        println!("  consent   {}", s.consent_version);
                        println!("  since     {}", s.consented_at);
                        if let Some(t) = s.last_push_at {
                            println!("  last push {t}");
                        }
                    } else {
                        println!("  enable: grid compliance enable");
                    }
                }
            }
            ComplianceCmd::Push { realm, json } => {
                let r = grid::gp::compliance_push(&config_dir, &realm).await?;
                if json {
                    println!("{}", serde_json::to_string_pretty(&r)?);
                } else {
                    println!("✓ {}", r.message);
                    println!("  realm {}", r.realm);
                    println!("  id    {}", r.gp_id);
                }
            }
        },
    }

    Ok(())
}

async fn run_wallet(config_dir: &PathBuf, action: WalletCmd) -> Result<()> {
    use grid::address::is_valid_address;
    use grid::chain::{ChainState, MAX_SUPPLY};
    use grid::wallet::{
        burn_check, claim_to_wallet, print_burn_banner, print_history, print_status, receive_tx,
        send, wallet_init, WalletMeta, CLAIM_DEADLINE_DAYS,
    };

    match action {
        WalletCmd::Init => {
            print_burn_banner();
            let w = wallet_init(config_dir).await?;
            println!("✓ wallet keys bound to chain");
            println!("  address: {}", w.address);
            println!("  meta:    {}", config_dir.join("wallet.json").display());
            println!("  chain:   {}", config_dir.join("chain.json").display());
            println!();
            println!("Balances & burns live on the **blockchain** (chain.json).");
            println!(
                "Unclaimed mint burns after {CLAIM_DEADLINE_DAYS}d via protocol — not wallet."
            );
            println!("  grid wallet claim");
        }
        WalletCmd::Status => {
            print_status(config_dir)?;
        }
        WalletCmd::Address => {
            let w = WalletMeta::load(config_dir)?;
            println!("{}", w.address);
            if !is_valid_address(&w.address) {
                anyhow::bail!("stored address failed validation");
            }
            let chain = ChainState::load(config_dir)?;
            println!("# on-chain balance: {:.6} GRID", chain.balance(&w.address));
            println!("# claim unclaimed mint: grid wallet claim");
            println!("# protocol burns unclaimed lots after {CLAIM_DEADLINE_DAYS}d (chain rule)");
        }
        WalletCmd::Claim { amount } => {
            print_burn_banner();
            let claimed = claim_to_wallet(config_dir, amount).await?;
            let w = WalletMeta::load(config_dir)?;
            let chain = ChainState::load(config_dir)?;
            println!("✓ claimed {claimed:.6} GRID on-chain → {}", w.address);
            println!("  on-chain balance: {:.6} GRID", chain.balance(&w.address));
        }
        WalletCmd::Send { to, amount, memo } => {
            print_burn_banner();
            let tx = send(config_dir, &to, amount, memo).await?;
            println!("✓ on-chain send {:.6} GRID", tx.amount);
            println!("  id:   {}", tx.id);
            println!("  from: {}", tx.from.as_deref().unwrap_or("-"));
            println!("  to:   {}", tx.to.as_deref().unwrap_or("-"));
            println!();
            println!("Counterparty: grid wallet receive-tx <tx.json> --from-pubkey <pk>");
        }
        WalletCmd::Receive => {
            print_burn_banner();
            let w = WalletMeta::load(config_dir)?;
            let chain = ChainState::load(config_dir)?;
            println!("Receive address (grid0):");
            println!("  {}", w.address);
            println!("  on-chain: {:.6} GRID", chain.balance(&w.address));
            println!();
            println!("  1) grid wallet claim        — unclaimed mint → your account (chain)");
            println!("  2) grid wallet receive-tx   — import signed send");
            println!();
            println!("Burn is chain protocol only (unclaimed lots >{CLAIM_DEADLINE_DAYS}d).");
            println!("Prefer exit to BTC / fiat.");
        }
        WalletCmd::ReceiveTx { path, from_pubkey } => {
            let raw = if path == "-" {
                let mut s = String::new();
                std::io::Read::read_to_string(&mut std::io::stdin(), &mut s)?;
                s
            } else {
                std::fs::read_to_string(&path)?
            };
            let tx: grid::wallet::WalletTx = serde_json::from_str(&raw)?;
            receive_tx(config_dir, tx, from_pubkey.as_deref())?;
            let w = WalletMeta::load(config_dir)?;
            let chain = ChainState::load(config_dir)?;
            println!("✓ receive applied on-chain");
            println!("  balance: {:.6} GRID", chain.balance(&w.address));
        }
        WalletCmd::History { limit } => {
            print_history(config_dir, limit)?;
        }
        WalletCmd::BurnCheck { dry_run } => {
            print_burn_banner();
            println!("Protocol burns execute on the **blockchain** (not wallet/node).");
            println!("Hard cap: {MAX_SUPPLY:.0} GRID");
            let _ = burn_check(config_dir, dry_run)?;
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
        AuthCmd::Nocrypt => auth_init(config_dir, AuthMode::Nocrypt).await?,
        AuthCmd::Login => auth_login(config_dir).await?,
        AuthCmd::Status => {
            let s = auth_status(config_dir);
            println!("GRID auth status");
            println!("  mode:       {}", s.mode);
            println!("  encrypted:  {}", s.keys_encrypted);
            println!("  unlocked:   {}", s.session_unlocked);
            println!("  passkey:    {}", s.passkey_registered);
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
        export_operational_key, export_pubkey_hex, generate_protected, load_authority,
        load_protected, run_genesis_server, store::fetch_truth, GenesisStore,
    };

    match action {
        GenesisCmd::Init => {
            banner::print_banner();
            println!();
            let dek =
                grid::passkey::require_identity(config_dir, "create protected genesis authority")
                    .await?;
            let authority = generate_protected(config_dir, &dek)?;
            let keys = load_protected(config_dir, &dek)?;
            let replica = grid::blockchain::ChainReplica::create_genesis(
                &keys,
                authority.recovery_pubkeys.clone(),
            )?;
            replica.save(config_dir)?;
            println!("✓ Protected genesis authority created");
            println!("  leader public: {}", authority.leader_pubkey);
            println!("  chain id:      {}", replica.chain_id);
            println!("  recovery:      2-of-2 public keys committed for genesis");
            println!(
                "  private keys:  {} (encrypted by combo vault)",
                config_dir.join("genesis").display()
            );
            println!();
            println!("Next:");
            println!("  grid genesis serve --bind 0.0.0.0:9100");
            println!("  grid genesis track --id <peer> --name <n> --listen host:port");
            println!();
            println!("Distribute ONLY the public key to peers:");
            println!("  export GRID_GENESIS_PUBKEY={}", authority.leader_pubkey);
        }
        GenesisCmd::ChainInit { operational } => {
            if grid::blockchain::ChainReplica::load(config_dir)?.is_some() {
                anyhow::bail!("block genesis already exists");
            }
            let authority = load_authority(config_dir)?;
            let keys = if operational {
                grid::genesis::load_keypair(config_dir)?
            } else {
                let dek =
                    grid::passkey::require_identity(config_dir, "create signed block genesis")
                        .await?;
                load_protected(config_dir, &dek)?
            };
            if authority.leader_pubkey != keys.public_hex() {
                anyhow::bail!("operational signer does not match Genesis authority");
            }
            let replica =
                grid::blockchain::ChainReplica::create_genesis(&keys, authority.recovery_pubkeys)?;
            replica.save(config_dir)?;
            println!("✓ signed block genesis created: {}", replica.chain_id);
        }
        GenesisCmd::Serve { bind, operational } => {
            banner::print_banner();
            println!();
            let keys = if operational {
                grid::genesis::load_keypair(config_dir)?
            } else {
                let dek = grid::passkey::require_identity(
                    config_dir,
                    "serve protected genesis authority",
                )
                .await?;
                load_protected(config_dir, &dek)?
            };
            run_genesis_server(config_dir.clone(), &bind, keys).await?;
        }
        GenesisCmd::Produce {
            coordinator,
            poll_secs,
            batch,
            operational,
            once,
        } => {
            let secret = std::env::var("GRID_CHAIN_SECRET")
                .map_err(|_| anyhow::anyhow!("GRID_CHAIN_SECRET is required"))?;
            let keys = if operational {
                grid::genesis::load_keypair(config_dir)?
            } else {
                let dek = grid::passkey::require_identity(config_dir, "produce signed GRID blocks")
                    .await?;
                load_protected(config_dir, &dek)?
            };
            grid::genesis::run_block_producer(
                keys,
                grid::genesis::ProducerOptions {
                    config_dir: config_dir.clone(),
                    coordinator,
                    secret,
                    poll: Duration::from_secs(poll_secs.max(1)),
                    batch,
                    once,
                },
            )
            .await?;
        }
        GenesisCmd::Track {
            id,
            name,
            listen,
            class,
        } => {
            let dek = grid::passkey::require_identity(config_dir, "track genesis peer").await?;
            let mut store =
                GenesisStore::open_with_keys(config_dir, load_protected(config_dir, &dek)?)?;
            store.track(&id, &name, &listen, &class)?;
            println!("✓ tracked {id} epoch={}", store.epoch());
        }
        GenesisCmd::Untrack { id } => {
            let dek = grid::passkey::require_identity(config_dir, "untrack genesis peer").await?;
            let mut store =
                GenesisStore::open_with_keys(config_dir, load_protected(config_dir, &dek)?)?;
            if store.untrack(&id)? {
                println!("✓ untracked {id} epoch={}", store.epoch());
            } else {
                println!("peer {id} was not tracked");
            }
        }
        GenesisCmd::Ban { id, reason } => {
            // Requires unlocked vault / passkey session when auth is initialized.
            let dek = grid::passkey::require_identity(config_dir, "policy mutation").await?;
            let mut store =
                GenesisStore::open_with_keys(config_dir, load_protected(config_dir, &dek)?)?;
            let id = grid::passkey::normalize_peer_target(&id);
            let rec = store.ban(&id, &reason)?;
            println!("✓ policy applied {}", rec.peer_id);
            println!("  epoch   {}", store.epoch());
        }
        GenesisCmd::Unban { id } => {
            let dek = grid::passkey::require_identity(config_dir, "policy mutation").await?;
            let mut store =
                GenesisStore::open_with_keys(config_dir, load_protected(config_dir, &dek)?)?;
            let id = grid::passkey::normalize_peer_target(&id);
            if store.unban(&id)? {
                println!("✓ policy cleared {id} epoch={}", store.epoch());
            } else {
                println!("no policy entry for {id}");
            }
        }
        GenesisCmd::List => {
            let dek =
                grid::passkey::require_identity(config_dir, "read protected genesis authority")
                    .await?;
            let store =
                GenesisStore::open_with_keys(config_dir, load_protected(config_dir, &dek)?)?;
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
        GenesisCmd::ExportLeader { output } => {
            let dek =
                grid::passkey::require_identity(config_dir, "export Genesis operational leader")
                    .await?;
            export_operational_key(config_dir, &dek, &output)?;
            println!("✓ operational leader exported to {}", output.display());
            println!("  recovery keys remain encrypted on this Mac");
        }
        GenesisCmd::Truth { url, pubkey } => {
            let t = fetch_truth(&url, pubkey.as_deref()).await?;
            println!("✓ signature valid");
            println!("epoch={} issued={}", t.body.epoch, t.body.issued_at);
            println!("genesis_pubkey={}", t.body.genesis_pubkey);
            println!(
                "tracked={} banned={}",
                t.body.tracked.len(),
                t.body.banned.len()
            );
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

/// GRID Engine phase 1 is deliberately P2P-only. A YAML manifest must never
/// silently start the host or mining loops.
async fn run_engine_peer(
    cfg: NodeConfig,
    config_dir: PathBuf,
    listen: String,
    mut connect: Vec<String>,
) -> Result<()> {
    if !connect.iter().any(|peer| peer == DEFAULT_GENESIS_PEER) {
        connect.insert(0, DEFAULT_GENESIS_PEER.to_string());
    }
    let dek = grid::passkey::require_unlocked(&config_dir, "start encrypted GRID Engine P2P peer").await?;
    let noise_static_key = grid::passkey::p2p_noise_static_key(&config_dir, &dek)?;
    println!("GRID Engine starts P2P only — host and mine remain off");
    run_peer(PeerOptions {
        node_id: cfg.node_id,
        name: cfg.name,
        class: cfg.class.to_string(),
        listen,
        connect,
        score: 0.0,
        genesis_url: Some(DEFAULT_GENESIS_URL.to_string()),
        genesis_pubkey: Some(DEFAULT_GENESIS_PUBKEY.to_string()),
        gp_id: None,
        realm: None,
        pubkey_hex: grid::passkey::operator_pubkey_hex(&config_dir).ok(),
        noise_static_key,
        config_dir,
    }).await
}

/// The canonical host path used by both `grid host` and `grid engine start`.
/// It intentionally never starts `grid mine`.
async fn run_engine_host(cfg: NodeConfig, compute: Option<String>) -> Result<()> {
    if !compute::containerd_available().await {
        anyhow::bail!(
            "GRID Engine runtime is not ready. Run `grid engine install`, then retry `grid host`."
        );
    }
    println!("GRID Engine host — approved isolated containers only · mining off");
    run_host(cfg, compute).await
}

/// Miner role: independently replicate/validate signed blocks over P2P while
/// doing only PoR work. No container host loop is started here.
async fn run_miner_node(
    cfg: NodeConfig,
    config_dir: PathBuf,
    listen: String,
    mut connect: Vec<String>,
    no_genesis: bool,
    noise_key_file: Option<PathBuf>,
) -> Result<()> {
    if !no_genesis && !connect.iter().any(|peer| peer == DEFAULT_GENESIS_PEER) {
        connect.insert(0, DEFAULT_GENESIS_PEER.to_string());
    }
    let noise_static_key = if let Some(path) = noise_key_file {
        let raw = std::fs::read(&path)?;
        raw.as_slice().try_into().map_err(|_| anyhow::anyhow!("P2P Noise key must be exactly 32 bytes"))?
    } else {
        let dek = grid::passkey::require_unlocked(&config_dir, "start encrypted P2P miner").await?;
        grid::passkey::p2p_noise_static_key(&config_dir, &dek)?
    };
    let peer = PeerOptions {
        node_id: cfg.node_id.clone(),
        name: cfg.name.clone(),
        class: cfg.class.to_string(),
        listen,
        connect,
        score: 0.0,
        genesis_url: Some(DEFAULT_GENESIS_URL.to_string()),
        genesis_pubkey: Some(DEFAULT_GENESIS_PUBKEY.to_string()),
        gp_id: None,
        realm: None,
        pubkey_hex: grid::passkey::operator_pubkey_hex(&config_dir).ok(),
        noise_static_key,
        config_dir,
    };
    println!("GRID MINER includes P2P block verification + PoR mining (host off)");
    tokio::select! {
        result = run_peer(peer) => result,
        result = run_mine(cfg) => result,
    }
}

async fn run_combined_node(
    cfg: NodeConfig,
    config_dir: PathBuf,
    listen: String,
    mut connect: Vec<String>,
    no_genesis: bool,
    noise_key_file: Option<PathBuf>,
) -> Result<()> {
    if !no_genesis && !connect.iter().any(|peer| peer == DEFAULT_GENESIS_PEER) {
        connect.insert(0, DEFAULT_GENESIS_PEER.to_string());
    }

    let noise_static_key = if let Some(path) = noise_key_file {
        let raw = std::fs::read(&path)?;
        raw.as_slice()
            .try_into()
            .map_err(|_| anyhow::anyhow!("P2P Noise key must be exactly 32 bytes"))?
    } else {
        let dek = grid::passkey::require_unlocked(&config_dir, "start encrypted P2P node").await?;
        grid::passkey::p2p_noise_static_key(&config_dir, &dek)?
    };

    let peer = PeerOptions {
        node_id: cfg.node_id.clone(),
        name: cfg.name.clone(),
        class: cfg.class.to_string(),
        listen,
        connect,
        score: 0.0,
        genesis_url: Some(DEFAULT_GENESIS_URL.to_string()),
        genesis_pubkey: Some(DEFAULT_GENESIS_PUBKEY.to_string()),
        gp_id: None,
        realm: None,
        pubkey_hex: grid::passkey::operator_pubkey_hex(&config_dir).ok(),
        noise_static_key,
        config_dir,
    };

    println!("GRID NODE includes P2P peer + host + mine");
    tokio::select! {
        result = run_peer(peer) => result,
        result = run_node(cfg) => result,
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
                .unwrap_or_else(|| "https://coordinator.grid-compute.com".into()),
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
