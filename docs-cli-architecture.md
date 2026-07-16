# GRID CLI - Technical Architecture & Implementation Guide

**Version:** 0.1.0 (Phase 1 MVP)  
**Last Updated:** July 2026  
**Status:** Early MVP - Core scaffolding complete, ready for Phase 2 integration

---

## Table of Contents

1. [Overview](#overview)
2. [Tech Stack](#tech-stack)
3. [Architecture Layers](#architecture-layers)
4. [Phase 1: MVP Implementation](#phase-1-mvp-implementation)
5. [Phase 2: Networking & Execution](#phase-2-networking--execution)
6. [Phase 3: Distributed Consensus](#phase-3-distributed-consensus)
7. [Phase 4: Mainnet & Scale](#phase-4-mainnet--scale)
8. [Token Economics](#token-economics)
9. [Security Model](#security-model)
10. [Operations & Deployment](#operations--deployment)

---

## Overview

**GRID CLI** is a distributed compute node for the GRID planetary supercomputer—a decentralized GPU/CPU resource marketplace. Nodes contribute compute capacity, earn GRID tokens based on **Proof-of-Resource** (PoR), and participate in a merit-based economy where early contributors build real value.

**Core Philosophy:**
- **Zero airdrop**: Pure meritocracy. Nodes earn only by proving resources and doing work.
- **Hybrid staking**: Lock GRID tokens to reduce slashing penalties and earn reward bonuses.
- **Multi-format execution**: WASM (safe/deterministic), Docker (flexible), Native (fast).
- **Distributed coordination**: No central hub; task gossip via P2P, consensus on blockchain.

---

## Tech Stack

### Language & Runtime
- **Rust** (2021 edition): Memory safety, zero-cost abstractions, excellent async support
- **Tokio**: Async runtime for concurrent task handling (10,000+ concurrent connections)
- **Futures**: Composable async patterns

### Blockchain & Cryptography
- **Ed25519-Dalek** (v2.1): Elliptic curve signatures for transaction signing
- **Blake3** (v1.5): Fast cryptographic hashing (256-bit output)
- **Secp256k1** (v0.28): Optional Bitcoin-compatible signing (Phase 2)
- **SHA-2** (v0.10): Legacy hashing support

### Networking & P2P (Phase 2+)
- **Tokio-Tungstenite** (v0.21): WebSocket client/server (Phase 1 fallback)
- **libp2p** (v0.53): Peer discovery, gossip protocol, DHT (Phase 2)
- **Hyper** (v1.0): HTTP/1.1 JSON-RPC for local APIs

### Execution Runtimes
- **Wasmtime** (v17.0): WASM execution engine with Cranelift compiler
- **Wasmtime-WASI** (v17.0): System interface for sandboxed I/O

### Database & Storage
- **RocksDB** (v0.21): High-performance key-value store (task cache, metrics)
- **SQLite** (v0.31 via rusqlite): Node state, ledger, configuration
- **Bincode** (v1.3): Binary serialization for blockchain transactions

### System Monitoring
- **sysinfo** (v0.30): Cross-platform CPU/memory/disk metrics
- **Prometheus** (v0.13): Metrics collection and export
- **Tracing** (v0.1): Structured logging with JSON output

### CLI & Configuration
- **Clap** (v4.4): Command-line parser with derive macros
- **TOML** (v0.8): Configuration file format (~/.grid/config.toml)
- **Config** (v0.13): Hierarchical config merging

### Dependencies Summary
- **40 direct dependencies**, ~200 transitive
- Binary size: ~80MB (unstripped), ~15MB (stripped release)
- Compile time: ~25 seconds (cold build on Apple Silicon)

**Excluded (Phase 2+):**
- `cudarc` (NVIDIA CUDA): Requires CUDA SDK (not available macOS dev environments)
- `procfs` (Linux-only): Use cross-platform `sysinfo` for MVP
- `hip-sys` (AMD HIP): Not stable on crates.io

---

## Architecture Layers

### Layer 1: CLI Interface (`src/main.rs`, `src/cli/commands.rs`)
**Responsibility:** User interaction & command routing

```
grid init                    → Initialize new node
grid daemon start/stop       → Node lifecycle
grid wallet balance/stake    → Token operations
grid resources all/benchmark → System diagnostics
grid status                  → Health check
```

**Commands Implemented (Phase 1):**
- `init --name "node-1" --tier compute-gpu --region us-west-2`
- `daemon start|stop|restart|status`
- `wallet balance|send|stake|unstake|mint`
- `resources cpu|gpu|storage|network|all|benchmark`
- `status`
- `test`

**Architecture:**
```
┌─────────────────────┐
│   CLI (main.rs)     │
│  Clap Parser        │
└──────────┬──────────┘
           │
     ┌─────┴─────┬──────────┬────────────────┐
     ▼           ▼          ▼                ▼
  [Init]    [Daemon]    [Wallet]       [Resources]
     │           │          │                │
     └─────────┬─┴──────────┴────────────────┘
               ▼
        [Node State Machine]
```

### Layer 2: Node State Machine (`src/node/mod.rs`)
**Responsibility:** Node lifecycle, configuration, event dispatch

```rust
pub struct Node {
    id: Vec<u8>,              // Ed25519 public key
    tier: NodeTier,           // compute-balanced, compute-gpu, storage, memory
    region: String,           // us-west-2, eu-central-1
    state: NodeState,         // Running, Paused, Maintenance
    config: NodeConfig,       // ~/.grid/config.toml
    blockchain: Blockchain,   // Token ledger
    resource_monitor: ResourceMonitor,
    reputation: u64,          // Earned through work
}
```

**States:**
- `Initializing` → Reading config, validating keys
- `Running` → Active, accepting tasks
- `Paused` → Disabled temporarily
- `Maintenance` → Offline for upgrades
- `Slashed` → Penalties applied (if malicious detected)

### Layer 3: Blockchain State Machine (`src/blockchain/mod.rs`)
**Responsibility:** Token ledger, transaction validation, consensus state

**Data Structures:**
```rust
pub struct Blockchain {
    pub blocks: Vec<Block>,           // Immutable block history
    pub balances: HashMap<NodeId, u128>,  // GRID token balances
    pub stakes: HashMap<NodeId, u128>,    // Locked tokens for staking
    pub total_supply: u128,           // Minted GRID total
    pub nonces: HashMap<NodeId, u64>, // Replay attack prevention
}

pub enum Transaction {
    MintToken { node_id, amount },
    TransferToken { from, to, amount, nonce },
    UpdateReputation { node_id, delta, reason },
    SlashNode { node_id, amount },
}
```

**Phase 1 Implementation:**
- Simple HashMap-based ledger (in-memory)
- Serial transaction application (no parallelization)
- Unit tests for mint, transfer, stake operations
- **Phase 2:** Persistence to RocksDB, proper PoA consensus

### Layer 4: Resource Monitoring (`src/resource/mod.rs`)
**Responsibility:** System metric collection & Proof-of-Resource scoring

**Metrics Collected:**
```rust
pub struct ResourceMetrics {
    cpu_flops: f64,              // CPU compute capacity (GFLOPS)
    gpu_flops: f64,              // GPU compute capacity (GFLOPS)
    memory_available_gb: f64,    // Free RAM
    memory_used_gb: f64,         // Used RAM
    storage_total_gb: f64,       // Disk space
    storage_used_gb: f64,        // Used disk
    network_latency_ms: u64,     // Ping to network
    network_bandwidth_mbps: f64, // Throughput
    uptime_percent: f64,         // Availability
    temperature_c: f32,          // CPU temp
    power_watts: f32,            // System power draw
}
```

**Proof-of-Resource Scoring Algorithm:**
```
composite_score = 
    cpu_score(0.35) +
    gpu_score(0.35) +
    storage_score(0.15) +
    uptime_score(0.10) +
    efficiency_score(0.03) +
    latency_score(0.02)

Where:
  cpu_score = min(cpu_flops / 400 GFLOPS, 1.0)
  gpu_score = min(gpu_flops / 1000 GFLOPS, 1.0)
  storage_score = min(storage_total / 2TB, 1.0)
  uptime_score = uptime_percent / 100
  efficiency_score = (flops / watts) / 5.0
  latency_score = max(1.0 - latency_ms / 100, 0.0)
```

**Phase 1:** Stub implementations (hardcoded CPU GFLOPS)  
**Phase 2:** Real metric collection via sysinfo, HWinfo APIs

### Layer 5: Token Economics (`src/token/mod.rs`)
**Responsibility:** Reward calculation, incentive design, tokenomics enforcement

**Dynamic Reward Formula:**
```
reward_per_hour = base_reward * demand_multiplier * efficiency_bonus * reputation_bonus * staking_bonus

Where:
  base_reward = 10 GRID per GPU-hour
  
  demand_multiplier (based on pending_tasks / available_capacity):
    pending > capacity * 2.0  → 5.0x (critical shortage)
    pending > capacity * 1.5  → 3.0x (high demand)
    pending > capacity * 1.0  → 2.0x (above supply)
    pending ≈ capacity        → 1.0x (balanced)
    pending < capacity * 0.5  → 0.5x (oversupply)
    pending << capacity       → 0.1x (massive oversupply)
  
  efficiency_bonus = 1.0 + (flops_per_watt / baseline) * 0.2  [0% to +20%]
  reputation_bonus = 1.0 + (reputation_score / max) * 0.3    [0% to +30%]
  staking_bonus = 1.0 + (staked_tokens / balance) * 0.5      [0% to +50%]
```

**Supply Cap:** 1 billion GRID tokens (hard limit)

**Phase 1 Constants:**
```toml
[tokenomics]
base_reward_per_gpu_hour = 10
max_supply_grid = 1_000_000_000
inflation_rate = 0.05  # 5% annual (Phase 2)
```

### Layer 6: Task Execution (`src/executor/mod.rs`)
**Responsibility:** Task lifecycle, sandboxing, output validation

**Execution Models (Phase 1 stubs):**
```rust
pub enum Executor {
    WASM {
        instance: WasmtimeInstance,
        memory_limit_mb: u32,
        timeout_sec: u64,
    },
    Docker {
        image: String,
        cpu_limit: u32,
        memory_limit_mb: u32,
    },
    Native {
        binary_path: String,
        args: Vec<String>,
    },
}
```

**Task Lifecycle:**
```
Submitted → Validated → Queued → Assigned → Running → Verified → Settled
```

**Phase 1:** Basic structure  
**Phase 2:** Full WASM/Docker execution with sandboxing

### Layer 7: Networking (`src/network/mod.rs`)
**Responsibility:** P2P communication, peer discovery, message routing

**Phase 1 Stub:**
```rust
pub struct P2PNode {
    local_peer_id: Vec<u8>,
    peers: HashMap<Vec<u8>, PeerInfo>,
    inbound_handler: WebSocketServer,  // Temporary
}
```

**Phase 2 libp2p Integration:**
```
libp2p
├── /ip4/127.0.0.1/tcp/30333/ws
├── Protocols
│   ├── /grid/1.0/gossip       # Task announcements
│   ├── /grid/1.0/consensus    # Blockchain sync
│   └── /grid/1.0/resource-ads # Resource proofs
├── DHT                          # Peer discovery
└── Reputation (use PeerScore)
```

### Layer 8: Security & Cryptography (`src/security/mod.rs`)
**Responsibility:** Key generation, signing, hashing

```rust
pub struct Crypto;

impl Crypto {
    pub fn generate_keypair() -> (Vec<u8>, Vec<u8>)  // (private, public)
    pub fn sign(private_key: &[u8], msg: &[u8]) -> Vec<u8>  // Ed25519
    pub fn hash(data: &[u8]) -> Vec<u8>  // Blake3
}
```

**Usage:**
- Node identity = public key (32 bytes)
- Transactions signed with private key
- All blocks/transactions hashed with Blake3

---

## Phase 1: MVP Implementation

### ✅ Completed

**CLI Framework (main.rs)**
- Full clap command structure
- 10+ commands wired to modules
- Config directory handling (~/.grid)
- Log level control

**Blockchain Module**
- In-memory HashMap ledger
- Token mint/transfer/stake/unstake
- Unit tests (100% coverage of core operations)
- Nonce tracking for replay attack prevention

**Resource Monitoring**
- System metrics collection framework
- Proof-of-Resource scoring algorithm
- Composite score calculation (0.0-1.0)

**Token Economics**
- Dynamic reward pricing engine
- Demand multiplier logic
- Efficiency/reputation/staking bonuses

**Cryptography**
- Ed25519 keypair generation
- Transaction signing (ready for Phase 2)
- Blake3 hashing

**Node State Machine**
- Configuration loading (TOML)
- Node initialization workflow
- CLI integration

### 🔄 Phase 1 Remaining Tasks (1-2 weeks)

1. **Persistence Layer** (~2 days)
   - Save blockchain state to RocksDB
   - SQLite for node config & transaction history
   - Graceful shutdown & recovery

2. **Integration Tests** (~3 days)
   - CLI flow testing (init → status → wallet)
   - Blockchain transaction validation
   - Resource scoring accuracy

3. **Local Testnet** (~3 days)
   - Script to spawn 3-5 mock nodes locally
   - Shared in-memory blockchain for testing
   - Mock task submission & scoring

4. **Reputation System** (~2 days)
   - Track successful/failed task completions
   - Uptime verification (heartbeats)
   - Slashing for malicious behavior

5. **Configuration Validation** (~1 day)
   - Schema validation for config.toml
   - Hot-reload support (no restart needed)
   - Default configurations

6. **Documentation** (~2 days)
   - API reference for CLI commands
   - Node operator setup guide
   - Development guide for contributors

---

## Phase 2: Networking & Execution

### Timeline: Weeks 3-6 (after Phase 1 complete)

### 1. P2P Networking (libp2p Integration)
**Objective:** Nodes discover each other, exchange tasks & proofs

**Tasks:**
```
a) libp2p bootstrap
   - Spawn Kademlia DHT for peer discovery
   - Multi-address resolution (/ip4/.../tcp/...)
   - Bootstrap nodes hardcoded initially

b) Gossip Protocol
   - /grid/1.0/gossip for task announcements
   - /grid/1.0/consensus for blockchain sync
   - Message validation & rate limiting

c) Peer Connection Lifecycle
   - Outbound: dial peers, establish sessions
   - Inbound: accept connections, auth peer ID
   - Keep-alive pings every 30s
```

**Acceptance Criteria:**
- 5+ nodes can discover each other
- Task gossip reaches all peers < 5 seconds
- No message duplicates

### 2. WASM Task Execution
**Objective:** Execute arbitrary WASM tasks in sandboxed environment

**Tasks:**
```
a) Wasmtime Integration
   - Memory isolation (per-task limits)
   - Syscall interception (WASI)
   - Timeout enforcement (per-task)

b) Task Interface
   - Input serialization (JSON/MessagePack)
   - Output capture (stdout/stderr)
   - Exit code & resource usage tracking

c) Deterministic Execution
   - Reproducible task runs (no randomness)
   - Floating-point determinism
   - Measurement of execution cost (CPU cycles, memory)
```

**Acceptance Criteria:**
- Execute fib(30) WASM, verify output
- Catch OOM/timeout gracefully
- Cost calculation within 5% of actual

### 3. GPU Detection & Support
**Objective:** Detect NVIDIA/AMD/Intel GPUs and report capabilities

**Tasks:**
```
a) NVIDIA (cudarc FFI or direct CUDA)
   - Query via nvidia-smi or CUDA driver API
   - Detect compute capability, VRAM, cores

b) AMD (HIP or ROCm)
   - hipGetDeviceProperties() for info
   - AMD ROCm toolkit integration

c) Intel (oneAPI)
   - Query Arc/Data Center GPU properties
   - Level Zero API for metrics

d) GPU Metrics
   - VRAM free/used
   - Clock speed
   - Temperature (if available)
   - Power draw (if available)
```

**Acceptance Criteria:**
- Detect GPU on 3 platforms
- Report GFLOPS within 10% of spec
- Thermal data available

### 4. Distributed Coordinator
**Objective:** Decouple task routing from central authority

**Tasks:**
```
a) Task Gossip Protocol
   - Broadcast tasks to random 10-15 peers
   - Each peer scores task locally
   - Top scorers bid for task

b) Quorum Voting
   - Top 3-5 bidders race to execute
   - Outcome broadcast to network
   - Consensus on winner via blockchain

c) Fault Tolerance
   - Retry on peer disconnection
   - Task timeout → rebroadcast
   - Byzantine fault tolerance (next phase)
```

**Acceptance Criteria:**
- 5+ nodes successfully coordinate task execution
- Task completes on highest-scoring node
- Retry on failure works

---

## Phase 3: Distributed Consensus

### Timeline: Weeks 7-10 (after Phase 2)

### 1. Proof-of-Authority (PoA) Blockchain
**Objective:** Validator-based consensus for token ledger

**Tasks:**
```
a) Block Production
   - Selected validators produce blocks every 12s
   - Collect pending transactions
   - Include merkle root of tasks

b) Validator Rotation
   - Top 20 nodes by reputation become validators
   - Rotate weekly based on performance
   - Slashing for validator misbehavior

c) Finality
   - Deterministic finality after 2 blocks
   - 100-block checkpoints
```

**Acceptance Criteria:**
- 5+ validators produce consecutive blocks
- Token balances persist across restarts
- Network can survive 1 validator failure

### 2. Smart Contract Foundation (Optional Phase 3.5)
**Objective:** Enable programmable incentives

**Tasks:**
```
a) Minimal VM
   - WASM-based contract execution
   - State tree (key-value store)
   - Contract migration framework

b) Built-in Contracts
   - Token minting/transfer rules
   - Reputation update logic
   - Slashing conditions
```

### 3. Reputation System v2
**Objective:** Advanced reputation tracking on-chain

**Tasks:**
```
a) Proof-of-Completion
   - Node commits to task outcome
   - Random sampling of nodes verify
   - Rewards paid on verified completion

b) Slashing Framework
   - 10% slash for failed task completion
   - 5% slash if staked (hybrid model)
   - 50% slash for double-signing

c) Reputation Decay
   - Reputation decays 1% per day (no activity)
   - Reputational gains are logarithmic
```

---

## Phase 4: Mainnet & Scale

### Timeline: Months 4-6 (after Phase 3)

### 1. Token Launch
**Objective:** Real-world GRID token economy

**Tasks:**
```
a) Token Bridge
   - Staging → Mainnet migration
   - Bootstrap liquidity pools (Uniswap)

b) Exchange Listings
   - Partner with DEXs initially
   - Apply to CEXs (later)

c) Staking Contracts
   - Smart contract for delegation
   - Auto-compounding rewards
```

### 2. Network Security Hardening
**Objective:** Production-ready security

**Tasks:**
```
a) Encryption
   - TLS for all P2P connections
   - Transaction signing verification
   - Message authentication codes (MACs)

b) DDoS Protection
   - Rate limiting per peer
   - Proof-of-work for connection establishment
   - Circuit breakers for overload

c) Audit & Bug Bounty
   - Third-party security audit
   - Bug bounty program ($10k-$100k)
   - Incident response procedures
```

### 3. Scalability
**Objective:** Support 10,000+ nodes

**Tasks:**
```
a) Sharding
   - Partition task space by hash(task_id)
   - Each shard has own validators
   - Cross-shard communication protocol

b) Light Client
   - Header-only sync (not full blockchain)
   - Merkle proof verification
   - Mobile-friendly

c) Layer 2 (Rollups)
   - Batch task outcomes off-chain
   - Periodic settlement on-chain
```

---

## Token Economics

### Supply Model

**Phase 1-2:** No real supply cap initially; test economics
**Phase 3+:** Hard cap of 1 billion GRID tokens

### Emission Schedule

```
Year 1: 100M GRID  (10% of max supply)
Year 2: 50M GRID   (decay to 5%)
Year 3: 25M GRID   (decay to 2.5%)
Year 4+: 10M GRID  (minimum inflation 1%)

Total after 4 years: 185M GRID circulating
```

### Staking Economics

**Stake Level → Reward Multiplier:**
```
0 GRID staked          → 1.0x multiplier (base)
1,000+ GRID staked     → 1.2x multiplier
10,000+ GRID staked    → 1.3x multiplier
100,000+ GRID staked   → 1.5x multiplier
1M+ GRID staked        → 2.0x multiplier (max)
```

**Slashing Without Stake:** 10% penalty  
**Slashing With Stake:** 5% penalty (80% reduction)

**Unstaking Lock Period:** 21 days (cliff vesting)

### Token Allocation (Mainnet launch)

```
Community & Contributors  → 15%  (150M GRID)
Ecosystem Grants          → 10%  (100M GRID)
Protocol Treasury         → 10%  (100M GRID)
Team (4-year vesting)     → 10%  (100M GRID)
Remaining                 → 55%  (550M GRID) - to be emitted
```

### Burn Mechanism

**Optional (Phase 3+):**
- 1% of all transaction fees burned
- Reduces inflation over time
- Creates deflation if adoption > emission

---

## Security Model

### Threat Model

| Threat | Mitigation |
|--------|-----------|
| **Double-Signing** | Nonce tracking, slashing via blockchain |
| **DDoS (network)** | Rate limiting, reputation-based peer selection |
| **Task Manipulation** | Random sampling of output verification, merkle proofs |
| **Reputation Gaming** | Logarithmic reputation gains, decay for inactivity |
| **Validator Collusion** | Byzantine Fault Tolerance (Phase 3+), random validator rotation |
| **Sybil Attack** | Resource-based identity (GPU/CPU power required to participate) |

### Cryptographic Foundations

- **Signing:** Ed25519 (fast, 128-bit security)
- **Hashing:** Blake3 (parallelizable, 256-bit output)
- **Encryption (Phase 2):** ChaCha20-Poly1305 for P2P comms
- **Key Derivation:** Argon2 for password-based key encryption

### Sandboxing Strategy

| Execution Type | Sandbox | Isolation |
|---|---|---|
| **WASM** | Wasmtime (memory isolation) | Process boundary + memory limits |
| **Docker** | Docker container | Full OS isolation |
| **Native** | Cgroup limits | Process group + resource quotas |

---

## Operations & Deployment

### Node Startup Sequence

```bash
$ grid init --name "my-gpu-node" --tier compute-gpu --region us-west-2
→ Generate Ed25519 keypair
→ Create ~/.grid/config.toml
→ Initialize blockchain ledger
→ Mint 1,000 GRID tokens to node

$ grid daemon start
→ Load config from ~/.grid/config.toml
→ Restore blockchain state from RocksDB
→ Connect to peer network (libp2p bootstrap Phase 2+)
→ Start resource monitor
→ Listen for tasks via gossip protocol
→ Begin earning rewards

$ grid status
Node ID:    0x1f3c9a8d...
Tier:       compute-gpu
Region:     us-west-2
Balance:    1,000 GRID
Staked:     500 GRID
Reputation: 2,400
Uptime:     99.8%
```

### Configuration (~/.grid/config.toml)

```toml
[node]
name = "my-gpu-node"
tier = "compute-gpu"
region = "us-west-2"
private_key = "0x..."  # Ed25519 private key (encrypted)

[network]
listen_addr = "0.0.0.0:30333"
bootstrap_nodes = [
  "12D3Ko...@/ip4/159.69.114.1/tcp/30333",
  "12D3Ko...@/ip4/167.99.92.9/tcp/30333",
]

[blockchain]
genesis_block = "0x..."  # Blake3 hash of genesis

[tokenomics]
base_reward_per_gpu_hour = 10
max_supply_grid = 1_000_000_000

[executor]
wasm_memory_limit_mb = 512
docker_enabled = true
native_enabled = false
task_timeout_sec = 3600
```

### Metrics & Monitoring

**Prometheus Export** (Phase 2):
```
# CPU score
grid_resource_cpu_score 0.85
# GPU capacity
grid_resource_gpu_flops 1200.0
# Token balance
grid_wallet_balance_grid 1000
# Tasks completed
grid_executor_tasks_completed_total 42
# Reputation score
grid_node_reputation 2400
```

**Logs** (structured JSON):
```json
{
  "timestamp": "2026-07-14T05:14:36.255Z",
  "level": "INFO",
  "module": "executor",
  "event": "task_completed",
  "task_id": "0xabc123...",
  "duration_sec": 1.234,
  "reward_grid": 12.5
}
```

### Troubleshooting

| Issue | Solution |
|-------|----------|
| **Node won't start** | Check `~/.grid/config.toml` syntax; see logs with `--log-level debug` |
| **No peers found** | Verify internet connection; check firewall port 30333; wait 30s for DHT bootstrap |
| **Low reputation** | Complete more tasks; ensure uptime > 95% |
| **Tasks timing out** | Increase `task_timeout_sec` in config; check system load |

---

## Development Roadmap

### Immediate (This Week)
- [x] Scaffold Rust project with 8 modules
- [x] Implement CLI with 10+ commands
- [x] Blockchain state machine with token ledger
- [x] Resource monitoring framework
- [x] Token economics engine
- [ ] **Phase 1 finalization:** Persistence, tests, local multi-node pilot

### Short-term (Weeks 2-3)
- [ ] SQLite persistence for node state
- [ ] Integration tests for full CLI flow
- [ ] Local 3-node pilot
- [ ] Reputation system v1
- [ ] Documentation

### Medium-term (Weeks 4-6)
- [ ] libp2p networking (peer discovery, gossip)
- [ ] WASM task execution (via Wasmtime)
- [ ] GPU detection (NVIDIA/AMD/Intel)
- [ ] Distributed Coordinator (task routing)

### Long-term (Months 2-3+)
- [ ] PoA consensus & validator rotation
- [ ] Smart contracts for programmable incentives
- [ ] Mainnet token launch & exchange listings
- [ ] Security audit & bug bounty program
- [ ] Network scalability (sharding, light clients)

---

## Contributing

### Build from Source
```bash
cd ~/projects/grid-cli
cargo build --release
./target/release/grid-cli --help
```

### Running Tests
```bash
cargo test                    # All tests
cargo test blockchain::tests  # Specific module
cargo test -- --nocapture    # With output
```

### Code Style
- Format: `cargo fmt`
- Lint: `cargo clippy`
- No unsafe code except in crypto/network FFI

---

## References

- **Whitepaper:** `docs/whitepaper.pdf`
- **Architecture Decisions:** `docs/architecture-decisions.md`
- **Setup Guide:** `docs/setup.md`
- **API Reference:** `docs/api-reference.md` (Phase 2)

---

**Questions?** Open an issue or reach out to the GRID team.

*Last updated: 2026-07-14 | Phase 1 MVP (v0.1.0)*
