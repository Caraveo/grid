<p align="center">
  <img src="./images/project-header-banner.png" alt="GRID — Proof-of-Resource Compute Network" />
</p>

<h1 align="center">GRID</h1>

<p align="center">
  <strong>A proof-of-resource compute network.</strong><br />
  Verify signed settlement, contribute useful capacity, and operate a private-by-default peer.
</p>

<p align="center">
  <a href="https://grid-compute.com">Website</a> ·
  <a href="https://grid-compute.com/quick">Quick start</a> ·
  <a href="https://explorer.grid-compute.com">Explorer</a> ·
  <a href="https://docs.grid-compute.com">Docs</a> ·
  <a href="https://discord.gg/nVs7NBCuqZ">Discord</a>
</p>

[![Version](https://img.shields.io/badge/version-0.2.20-blue.svg)](https://github.com/Caraveo/grid/releases/tag/v0.2.20)
[![Platform](https://img.shields.io/badge/platform-macOS%20%7C%20Linux%20%7C%20Windows-blue.svg)](https://github.com/Caraveo/grid/releases)
[![Language](https://img.shields.io/badge/Rust-2021-orange.svg)](https://www.rust-lang.org/)
[![Network](https://img.shields.io/badge/network-Genesis%20pilot-ec8a26.svg)](https://explorer.grid-compute.com)

> **Canonical project:** This is the primary GRID repository. It contains the
> protocol, Rust CLI, P2P node, blockchain, mining, wallet, and Engine runtime.
> The related repositories below provide the public site, coordinator, MESH
> desktop client, and Solana integration.

> **Network status:** GRID is a public, Genesis-led production pilot. It is **not** decentralized mainnet consensus and must not be represented as such. A single Genesis leader currently finalizes canonical blocks; enforced independent validator quorum, audits, and hardened production operations remain launch gates.

## What GRID is

GRID is a Rust CLI and operator runtime for a compute network with four distinct roles:

| Role | Command | Does | Does not do |
| --- | --- | --- | --- |
| Peer | `grid peer` | Encrypted P2P discovery, block replication, signed-chain verification | Mine or host workloads |
| Miner | `grid mine` | Peer duties plus capped proof-of-resource work and settlement polling | Run customer containers |
| Host | `grid host` | Serve approved isolated workload requests through the Engine runtime | Mine proof-of-resource work |
| Node | `grid node` | Peer + miner + host together | Replace a validator quorum |

`grid engine start` is the host-only Engine entry point; `grid host` uses the same host path. `grid start` is an alias for the all-in-one node flow unless passed an Engine manifest.

### Current network components

```text
                         public, privacy-rounded telemetry
  Explorer  <──────────────────── Cloudflare registry
     │                                        ▲
     │ canonical status                       │ signed heartbeat (opt-in location)
     ▼                                        │
Genesis node ── signed blocks / P2P sync ── GRID peers and miners
     │                                        │
     └───────── coordinator ── verified settlements ── hosts
                                                   │
                                                   └── approved container work (staged)
```

- **Genesis** is the current trust anchor, canonical signed-block source, and ban-list source of truth.
- **Coordinator** assigns and verifies pilot proof-of-resource work and records settlement data.
- **P2P peers** use a Noise XX encrypted transport for GRID protocol messages and replicate/verify signed blocks.
- **Cloudflare registry and explorer** show network health and coarse, opt-in geographic telemetry; they are not a consensus authority.
- **GRID Engine** is the platform-aware container-host foundation. Its first approved workload profile is Git-backed Caddy with tunnel-only exposure.

## Start here

### Install a release

The installer selects the matching published binary, verifies the release artifacts, and installs `grid` into `~/.local/bin` by default.

```bash
curl -fsSL https://grid-compute.com/downloads/install.sh | bash
hash -r
grid --version
```

For reinstall/upgrade:

```bash
curl -fsSL https://grid-compute.com/downloads/install.sh | bash -s -- --force
```

For a system-wide installation (may request `sudo`):

```bash
curl -fsSL https://grid-compute.com/downloads/install.sh | bash -s -- --system --force
```

### Initialize an operator identity

```bash
grid init --name my-node --class S
grid auth keyphrase
```

`grid auth keyphrase` creates a 24-word recovery phrase. Store it offline. Do not paste it into chat, a ticket, a website, or a repository. Other local vault choices are available through `grid auth --help`.

### Choose one role

```bash
# P2P replica only: sync and verify, no mining and no hosted workload
grid peer --with-bench

# P2P replica plus proof-of-resource work; no container hosting
grid mine

# Approved isolated workload host; mining stays off
grid host

# One machine doing peer + mine + host duties
grid node
```

By default, peer-bearing commands dial the canonical Genesis peer and listen on TCP `9900`. Outbound-only operation works; forwarding TCP `9900` makes a node more useful to the mesh. Use `--no-genesis` / `--p2p-no-genesis` only when operating the Genesis side or an isolated test network.

`grid node` also exposes a read-only Phoenix wallet API on
`127.0.0.1:9100`. Phoenix's **Local node** mode uses that endpoint and P2P
`127.0.0.1:9900`. To allow Phoenix on another trusted machine to connect, bind
the API explicitly and advertise/forward the P2P listener:

```bash
grid node --p2p-listen 0.0.0.0:9900 --wallet-bind 0.0.0.0:9100
```

Then choose **Custom node** in Phoenix with `http://NODE_HOST:9100` and
`NODE_HOST:9900`. Restrict TCP `9100` and `9900` with the host/network firewall;
the wallet API is read-only, while P2P traffic is Noise encrypted.

For the concise operator guide, see [grid-compute.com/quick](https://grid-compute.com/quick).

## Operator basics

### Observe the node and the network

```bash
grid status                 # local configuration and host resources
grid resources              # CPU / memory sample
grid bench --duration 5     # local benchmark
grid stats                  # coordinator status and pilot earnings state
grid registry               # public privacy-preserving registry view
```

Live public views:

- [Explorer](https://explorer.grid-compute.com) — signed blocks, telemetry, capacity estimates, and coordinator status
- [Genesis health](https://genesis.grid-compute.com/health) — Genesis service health
- [Coordinator status](https://coordinator.grid-compute.com/v1/status) — settlement service status

### Opt into the globe

The globe is optional. GRID never needs latitude/longitude in order to peer, mine, or host. If you choose to appear, publish a deliberately coarse location only; the registry is designed not to display IP addresses, ports, wallets, or coordinator URLs.

Create `~/.grid/env` with permissions restricted to your user:

```bash
GRID_GLOBE_LAT=35.1
GRID_GLOBE_LNG=-106.6
GRID_GLOBE_REGION=NA-US-NM
```

Then restart the relevant `grid peer`, `grid mine`, or `grid node` process. The node signs registry heartbeats with a dedicated local key; private key material remains on the operator machine.

### Devnet reward wallet

The Solana path is a **development-network pilot** for testing reward settlement UX, not a public-market or mainnet launch.

```bash
grid solana create
grid solana status
```

Set a payout address only when you control that devnet wallet:

```bash
export GRID_SOLANA_REWARD_WALLET=YOUR_DEVNET_SOLANA_ADDRESS
grid mine
```

Verified work may be eligible for a coordinator-controlled pilot settlement. Uptime alone does not mint value, devnet assets have no economic value, and mainnet token issuance is intentionally outside this CLI flow.

## GRID Engine and hosting

GRID Engine prepares the runtime boundary for voluntary compute hosts. It is intentionally restrictive: containers must not receive the GRID vault, node keys, arbitrary host paths, broad capability sets, the container socket, or arbitrary host ports.

### Runtime preparation

```bash
grid engine doctor
grid engine install
grid engine start
```

Supported runtime approach:

| Platform | Host runtime |
| --- | --- |
| Linux | containerd + nerdctl + gocryptfs |
| macOS | Lima Linux VM + nerdctl + gocryptfs |
| Windows | Ubuntu in WSL2 + the Linux workflow |

`grid engine install` checks and installs the supported runtime prerequisites for the current platform. It may invoke the platform package manager or WSL setup, so read its output and approve only on machines you administer.

On macOS, the dedicated `grid-containerd` VM receives one writable host mount:
the selected GRID config directory's `engine/` subtree. The parent config
directory, wallet, vault, and keys are not mounted into the VM. Container
processes receive only the individual service checkout as a read-only mount.
Encrypted ciphertext persists in the shared Engine subtree; decrypted
gocryptfs mountpoints exist only on the VM-native filesystem. Ubuntu performs
the FUSE mount with guest administrator authority, then forces file ownership
to the rootless Engine user. The vault-wrapped volume key is sent through
standard input for the mount and immediately zeroized.

### Private Caddy service lifecycle

```bash
# Create a reviewable P2P Engine manifest; it does not start a workload.
grid engine init grid-engine.yaml --name grid-node

# Prepare key metadata for one encrypted service volume.
grid engine volume prepare my-site-data
grid engine volume status my-site-data

# Write and validate the narrow initial service contract.
grid engine service init-web site.yaml --name my-site --repo https://github.com/OWNER/REPO.git
grid engine service check site.yaml

# For one private Git repository, generate a vault-wrapped deploy key.
grid engine service key-create --name my-site --repo git@github.com:OWNER/REPO.git

# Deploy: encrypted mount → host-side Git fetch → isolated Caddy.
grid engine service deploy site.yaml
grid engine service status my-site
grid engine service logs my-site

# On the client, copy the `public` value.
grid auth status

# On the host, issue a one-time capability bound to that client identity.
grid engine service capability my-site \
  --client-pubkey CLIENT_OPERATOR_PUBLIC_KEY

# On an assigned client, open one loopback-only stream over encrypted GRID P2P.
grid engine service connect \
  --peer HOST:9900 \
  --name my-site \
  --bind 127.0.0.1:41784

# Stop Caddy and immediately unmount encrypted storage.
grid engine service stop my-site

# Explicit, permanent deletion of encrypted service content.
grid engine service destroy my-site --yes
```

The capability is entered through a hidden prompt unless
`GRID_ENGINE_CAPABILITY` is set. It is scoped to one service and consumed when
the host accepts the stream. The client and container are both loopback-only:
Engine does not create a DNS record, public URL, router rule, or wildcard
listener.

### First workload profile

```text
Git repository → host-side authenticated fetch → Caddy container → GRID tunnel
                         │                           │
                         └── vault/key material never enters container ──┘
```

The first profile allows only the approved `caddy:2-alpine` image, a Git
source, the fixed service port `41783`, and `grid-tunnel` exposure. The Git
checkout runs on the host inside a gocryptfs mount. Caddy receives a recursive
read-only bind of the checkout and never receives the GRID vault, deploy key,
encrypted-volume key, host root, or containerd socket. Runtime receipts include
the deployed commit and are signed by the operator key. Arbitrary images,
direct WAN/LAN exposure, host networking, and privileged settings are rejected.
All Linux capabilities are dropped except the official Caddy image's
`NET_BIND_SERVICE` file capability; `no-new-privileges` remains enabled and
Engine still binds only its fixed high port to host loopback.

## Security model and limits

### What is implemented

- Ed25519 identities and signed Genesis/settlement data
- Noise XX encrypted P2P transport for GRID protocol messages
- Local operator vault modes: passkey, password, keyphrase, combo, or explicit `nocrypt`
- Separate operator-vault and Genesis-authority key paths
- Signed, replay-resistant registry heartbeats with opt-in coarse geography
- P2P block synchronization followed by local signed-chain verification
- Private Engine Caddy lifecycle with gocryptfs storage, per-repository deploy keys, loopback-only networking, one-time P2P capabilities, and signed runtime receipts

### Important limits

- A direct P2P peer can observe the other peer's network address. GRID does not presently provide an IP-hiding relay.
- A conventional container runtime cannot prevent a host that executes a workload from seeing that workload's plaintext. Host-blind execution needs confidential-compute attestation (for example TDX or SEV-SNP), which is not enabled here.
- Genesis-led blocks are not a decentralized validator quorum. `grid mainnet` is intentionally fail-closed while this remains true.
- No public token price, exchange listing, or promise of monetary return is provided by this project.

## Mainnet launch gate

```bash
grid mainnet
grid mainnet --json
```

GRID should remain labelled a Genesis pilot until all of the following are complete:

1. Independent validators propose and verify blocks with enforced quorum certificates.
2. Validator keys, rotation, slashing/dispute procedures, and governance are documented and tested.
3. Block persistence is append-only, segmented, backed up, and recovery-tested.
4. Coordinator and Engine execution boundaries receive independent security review.
5. Public operators can bootstrap, sync, monitor, and recover without relying on a single private machine.
6. Token allocation, vesting, treasury controls, liquidity disclosures, and legal/compliance work are finalized separately from the devnet pilot.

`grid mainnet --migrate-storage` is a one-time, non-destructive storage migration helper for the block store. Do not use it as evidence that the launch gate has passed.

## Build from source

Requirements: a current stable [Rust toolchain](https://rustup.rs), Git, and platform build tools. Container runtime prerequisites are required only for host/Engine work.

```bash
git clone https://github.com/Caraveo/grid.git
cd grid
cargo build --release
./target/release/grid --help
```

Install a local build:

```bash
./scripts/install.sh --from-source --force
# or
make install
```

Run the test suite:

```bash
cargo test
cargo clippy --all-targets -- -D warnings
```

## Repository map

```text
src/                 Rust CLI, P2P, chain, coordinator, wallet, Engine, and policy modules
src/p2p/             Encrypted peer transport, discovery, sync, and gossip
src/blockchain.rs    Signed block and settlement-chain validation
src/engine.rs        Runtime checks, Engine manifests, encrypted-volume and service scaffolding
scripts/             Installer and platform helpers
deploy/              Deployment material, including AWS resources
docs/                Operator and implementation notes
wallet-apps/          Desktop and platform wallet work
contracts/            Solana/token-related project material
legacy/ts/            Historical TypeScript prototype; not the current CLI
```

Related repositories in the GRID workspace:

| Project | Purpose |
| --- | --- |
| [`grid-site`](https://github.com/Caraveo/grid-site) | Public website, explorer UI, documentation UI, and Cloudflare integration |
| [`grid-coordinator`](https://github.com/Caraveo/grid-coordinator) | Coordinator-facing service material |
| [`grid-net`](https://github.com/Caraveo/grid-net) | MESH desktop client, network tooling, and support code |
| [`grid-solona`](https://github.com/Caraveo/grid-solona) | Solana devnet token/reward experimentation |

## Documentation

- [Quick start](https://grid-compute.com/quick)
- [Proof of Resource](https://grid-compute.com/por)
- [Network explorer](https://explorer.grid-compute.com)
- [Public documentation](https://docs.grid-compute.com)
- [White paper](https://grid-compute.com/white-paper)
- [Token allocation](https://grid-compute.com/alloc)
- [Container Engine overview](https://engine.grid-compute.com)

Local project references, where present:

- [`technical.md`](./technical.md) — implementation notes and phases
- [`GRID_White_Paper.md`](./GRID_White_Paper.md) — project vision
- [`GRID_Token_Specification.md`](./GRID_Token_Specification.md) — token and settlement design
- [`OnePage.pdf`](./OnePage.pdf) — project one-pager
- [`scripts/install.sh`](./scripts/install.sh) — release installer

## Contributing and reporting security issues

Please open an issue or discussion before proposing a large protocol, token, or network-policy change. Never include seed phrases, private keys, vault contents, coordinator secrets, production credentials, or a vulnerable exploit proof of concept in an issue or pull request.

For a potential security vulnerability, contact the project maintainers privately through the official project channels rather than publishing an exploit path.

## License

MIT for code unless a file states otherwise. Documentation and branded material may carry their own notices.
