<p align="center">
  <img src="./logo.svg" alt="GRID" width="72" height="72" />
</p>

<h1 align="center">GRID</h1>

<p align="center">
  <strong>Useful mining</strong> for a planetary compute network.<br/>
  Run a node. Do real work. Earn <strong>GRID</strong>. Cash out toward <strong>Bitcoin</strong>.
</p>

[![macOS](https://img.shields.io/badge/platform-macOS-blue.svg)](https://www.apple.com/macos/)
[![Rust](https://img.shields.io/badge/Rust-2021-orange.svg)](https://www.rust-lang.org/)
[![Version](https://img.shields.io/badge/version-0.2.15-blue.svg)](https://github.com/Caraveo/grid/releases)
[![Status](https://img.shields.io/badge/status-PREALPHA-red.svg)](https://github.com/Caraveo/grid)

<p align="center">
  <em>Bitcoin is the Transact Security Layer</em> — GRID meters compute; BTC secures value.
</p>

---

## Get started now

### macOS

```bash
curl -fsSL https://raw.githubusercontent.com/Caraveo/grid/master/scripts/install.sh | bash
brew install lima
limactl start --name=grid-containerd template:default
nerdctl info
grid init --name my-node --class S
```

### Linux

```bash
curl -fsSL https://raw.githubusercontent.com/Caraveo/grid/master/scripts/install.sh | bash
# Install rootless containerd + nerdctl using your distribution or nerdctl release.
nerdctl info
grid init --name my-node --class S
```

### Windows (WSL2)

```powershell
wsl --install -d Ubuntu
wsl -d Ubuntu
```

Then, inside Ubuntu, follow the Linux commands above. GRID host jobs use the
same Linux containerd isolation in WSL2; a native Windows binary is published
for CLI use, but native Windows containers are intentionally not used for host
jobs.

## Install details

### One-liner (`curl`) — recommended

```bash
curl -fsSL https://raw.githubusercontent.com/Caraveo/grid/master/scripts/install.sh | bash
```

Then open a **new terminal** (or `hash -r`) and verify:

```bash
which grid && grid -V
grid auth --help      # must list passkey / combo / …
```

### Reinstall / upgrade / options

```bash
# Replace any existing binary (including legacy CLIs also named `grid`)
curl -fsSL https://raw.githubusercontent.com/Caraveo/grid/master/scripts/install.sh | bash -s -- --force

# Always cargo-build from git
curl -fsSL https://raw.githubusercontent.com/Caraveo/grid/master/scripts/install.sh | bash -s -- --from-source

# Prefer /usr/local/bin (may prompt for sudo)
curl -fsSL https://raw.githubusercontent.com/Caraveo/grid/master/scripts/install.sh | bash -s -- --system --force

# Custom directory
curl -fsSL https://raw.githubusercontent.com/Caraveo/grid/master/scripts/install.sh | bash -s -- --prefix="$HOME/bin"
```

### From a git clone

```bash
git clone https://github.com/Caraveo/grid.git
cd grid
./scripts/install.sh --local --force
# or
make install                 # → ~/.local/bin/grid
make install-system          # → /usr/local/bin/grid
```

### What the installer does

1. Tries a **GitHub Release** prebuilt (`grid-<os>-<arch>`) when available  
2. Otherwise installs **Rust** (if needed) and **`cargo build --release`**  
3. Installs to **`~/.local/bin/grid`** by default (or `--prefix` / `--system`)  
4. **Detects Phase 1** via `grid auth` — refuses/replaces legacy binaries that also used the name `grid`  
5. Backs up non-Phase-1 copies as `*.legacy.bak`  
6. Ensures `~/.local/bin` is on **PATH** (appends to `~/.zshrc` / `~/.bashrc` when safe)  
7. Writes `~/.grid/install-info.txt` and prints next steps  

If `grid auth` is “unrecognized”, you’re still on a **legacy** binary:

```bash
which -a grid
curl -fsSL https://raw.githubusercontent.com/Caraveo/grid/master/scripts/install.sh | bash -s -- --force --system
hash -r && grid auth --help
```

### Uninstall

```bash
curl -fsSL https://raw.githubusercontent.com/Caraveo/grid/master/scripts/install.sh | bash -s -- --uninstall
# or from a clone: make uninstall
```

Leaves `~/.grid/` config and keys alone.

Requires [Rust](https://rustup.rs) only when building from source (the installer can bootstrap rustup).

---

## Quick start — host + mine

| Track | Command | What | Earn |
|-------|---------|------|------|
| **Host** | `grid host` | Pull useful **container** jobs, serve isolated | **Higher** |
| **Mine** | `grid mine` | PoR / transactional-security work (`blake3_work`) | **Slower** |
| **Both** | `grid node` | Host + mine on one box | mixed |

```bash
# once
grid init --name garage --class S
grid auth                 # passkey default (or password / combo / …)

# terminal 1 — coordinator (auto mine PoR + demo host jobs)
grid coord --bind 0.0.0.0:8787

# terminal 2 — name a compute you own, then HOST useful work
grid launch garage --public          # default public; use --private for fabric-only
grid host                            # pull container_work · higher earn

# terminal 3 — optional MINE security work
grid mine                            # blake3_work · slower earn

# inspect
grid compute list
grid stats
grid wallet
```

Containers are **fully isolated** from the host (no host mounts, cap-drop ALL, resource limits). The host runtime is **containerd via nerdctl**; GRID refuses to fall back to Docker.

Runtime support: Linux runs rootless containerd/nerdctl directly. macOS uses a
dedicated rootless Lima Linux VM. Windows runs the identical Linux workflow in
WSL2; run `powershell -ExecutionPolicy Bypass -File scripts/install-runtime.ps1`
to check it. Native Windows containers are intentionally not used for GRID host
jobs because their isolation controls do not match the Linux contract.

### Launcher container access

An interactive job may request `"tunnel": true` with `"servicePort": 41783`.
This is the only GRID container service port. It is bound to `127.0.0.1` on
the compute host, never to a LAN/WAN interface; it is reserved for an assigned
launcher’s authenticated encrypted GRID peer session. Arbitrary host ports,
host networking, Docker socket mounts, and host shells are not permitted.

Launcher admission is tied to a 32-byte public key. Requests containing
Docker/Kubernetes host-escape controls (privileged mode, host networking/PID,
host paths, capabilities, or Docker socket access) are rejected and the
launcher key is permanently banned by the coordinator.

Transport design: a launcher and assigned remote node use their own ephemeral
X25519 session keys; GRID/MESH may hold a separate broker key for encrypted
locator and capability metadata, but that broker key must not decrypt workload
content. A standard container runtime cannot hide plaintext from the host that
executes it. Host-blind workloads therefore require verified confidential
compute attestation (TDX/SEV-SNP or equivalent) and are not enabled by this
pilot runtime.

### Public compute registry (grid-compute.com)

Hosts announce capacity to the site; anyone can check availability:

```bash
grid compute available              # free slots only
grid compute available --all        # include busy/offline
grid compute announce               # re-push local computes
grid registry                       # peers + compute stats
```

API: `GET https://grid-compute.com/api/registry/computes?available=1`  
Announce: `POST /api/registry/computes` (same webhook secret as mesh ping). No IPs stored.


Work kind: `blake3_work` payload `seed|iterations` (default 250k iterated BLAKE3).
Coordinator verifies by re-computing the digest. Credits land in `~/.grid/earn.json`
and `~/.grid/coord/state.json` (survive restarts).

Earnings are **disabled by default** for private-network safety. Jobs can be
verified without minting value. Do not enable `GRID_ENABLE_EARN=1` until signed
replica settlement has been independently validated and audited.

```bash
# optional: submit extra PoR yourself
grid submit --job blake3_work --wait
grid stats
```

### Benchmark this machine

```bash
grid bench
grid bench --duration 5
grid bench --json
```

### Public mesh registry — [grid-compute.com](https://grid-compute.com)

**grid-compute.com** is the network’s public peer registry (Cloudflare).  
Location-only — **never** IPs, ports, or endpoints.

```bash
grid registry              # list peers from https://grid-compute.com/api/registry
grid registry --json
```

**Join the registry** (opt-in coords) — create `~/.grid/env` (mode `600`):

```bash
# ~/.grid/env   (chmod 600)  — never commit this file
# GRID_SITE_URL defaults to https://grid-compute.com if unset
GRID_WEBHOOK_SECRET=...          # Cloudflare Worker secret (required in prod)
GRID_GLOBE_LAT=37.7
GRID_GLOBE_LNG=-122.4
GRID_GLOBE_REGION=NA-W
```

```bash
# ~/.grid/config.toml  (or env GRID_GLOBE_LAT / GRID_GLOBE_LNG)
# [node]
# globe_lat = 37.7
# globe_lng = -122.4
# globe_region = "NA-W"

grid node   # pings registry on start + every ~5m after heartbeat
```

Skip coords → mining continues; you just won’t appear on the globe/registry.

### Auth (protect operator keys)

```bash
grid auth                 # default = passkey
grid auth passkey
grid auth password
grid auth keyphrase       # 24-word BIP39 phrase
grid auth combo           # password → passkey → keyphrase
grid auth nocrypt         # plain keys only (0600)
grid auth login
grid auth status
grid auth delete --wipe-keys
```

Secrets live under `~/.grid/keys/` and `~/.grid/passkey/` — gitignored. Never commit them.

Operator vault modes are **not** required for genesis authority. Genesis uses
`grid genesis init` keys under `~/.grid/genesis/` — separate from the vault.

### Genesis registry

```bash
grid genesis init
grid genesis serve --bind 127.0.0.1:9100
grid genesis track --id bob-1 --name bob --listen 127.0.0.1:9901 --class S
```

### P2P mesh (GP discovery + encrypted transport)

```bash
# terminal A
grid peer --listen 127.0.0.1:9900 --with-bench

# terminal B
grid peer --listen 127.0.0.1:9901 --connect 127.0.0.1:9900 --with-bench
```

You should see **hello**, **pong rtt=… ms**, and a **peers** list.

Peer discovery uses the existing `grid-compute.com` GP directory when a realm
is supplied. The directory is a locator only; every TCP session completes a
Noise XX (`X25519 + ChaChaPoly + BLAKE2s`) handshake before GRID protocol
messages are sent. Direct peers can still observe each other’s network address;
an IP-hiding relay is a separate service and is not implied by GP naming.

| Command | What |
|---------|------|
| `grid coord` | Job coordinator |
| `grid node` | Miner — claim work, earn |
| `grid peer` | **P2P** listen/dial, hello, ping RTT, peer gossip |
| `grid auth` | Protect operator keys (passkey / password / 24-word / combo / nocrypt) |
| `grid registry` | **Public mesh registry** (grid-compute.com) |
| `grid genesis` | Phase 0 signed truth / ban list (local authority) |
| `grid bench` | **Benchmark** CPU hash + memory throughput |
| `grid init` | Write `~/.grid/config.toml` |
| `grid submit` | Submit allowlisted job (`echo`, `hash_file`) |
| `grid stats` | Jobs + nodes |
| `grid status` | Config + host metrics + Bitcoin TSL |
| `grid wallet` | Balance stub + GRID → BTC exit reminder |
| `grid resources` | CPU / memory sample |

---

## What Phase 1 is

**In (minimal MVP)**

- Single **Rust** `grid` binary  
- Jobs: `coord` + `node` + `submit` (verify + PoR earn)  
- **`grid bench`** — hash + memory scores  
- **`grid peer`** — TCP P2P hello / ping RTT / peer gossip  
- Class **S / M / L**, Bitcoin as **Transact Security Layer**

**Later**

- Docker / GPU kernels, Genesis Earn locks on-rail  
- libp2p / NAT traversal, critical-latency fabric  
- **Edge wallets** (below)

---

## Edge wallets (roadmap)

Operators and users need more than a CLI. **Edge wallets** are first-class clients that hold keys, show earn/balance, and move value **GRID → BTC** (TSL) without a custodial middleman where possible.

### Principles

| Principle | Meaning |
|-----------|---------|
| **Keys at the edge** | Seed / keys live on user device or user-controlled service HSM — not on the coordinator by default |
| **Bitcoin TSL** | Cash-out and high-value finality prefer **BTC**; GRID is utility for compute |
| **Same identity story** | Node id / operator cluster / wallet address link without forcing one UI |
| **Least custody** | Default non-custodial; optional “services” tier for teams who want managed ops |
| **No public testnet token** | Wallet product tracks **mainnet / Genesis Earn** economics only |

### Surface map

| Surface | Who | Role | Phase target |
|---------|-----|------|----------------|
| **Software (desktop)** | Miners, power users | Full node control: init, node, status, earn, export keys, GRID→BTC swap UX | **P2** thin wallet UI wrapping `grid`; **P3** full desktop wallet |
| **Mobile** | Operators on the go, small miners | Monitor earn, alerts, approve cash-out, watch-only node status; limited signing | **P3** watch + notify; **P4** sign + Lightning/BTC exit |
| **Web** | Buyers + light operators | Submit jobs, pay for capacity, view invoices, connect software/mobile wallet | **P2** buyer portal; **P3** wallet connect (WalletConnect-style or GRID session keys) |
| **Services** | Fleets, datacenters, studios | API keys, multi-node fleet, payroll to BTC, SSO, audit logs, optional custody | **P3+** operator API + service wallet policies |

```
┌──────────────┐  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐
│  Software    │  │    Mobile    │  │     Web      │  │   Services   │
│  desktop app │  │  iOS/Android │  │  buyer/ops   │  │  fleet API   │
└──────┬───────┘  └──────┬───────┘  └──────┬───────┘  └──────┬───────┘
       │                 │                 │                 │
       └─────────────────┴────────┬────────┴─────────────────┘
                                  ▼
                    ┌─────────────────────────┐
                    │  GRID edge wallet core  │
                    │  keys · earn · identity │
                    └───────────┬─────────────┘
                                │
              ┌─────────────────┼─────────────────┐
              ▼                 ▼                 ▼
        grid node / CLI   utility rail (GRID)   Bitcoin TSL
```

### Capabilities by surface (planned)

| Capability | Software | Mobile | Web | Services |
|------------|:--------:|:------:|:---:|:--------:|
| Run / control local node | ● | ○ | — | ● (remote agents) |
| View earn / PoR score | ● | ● | ● | ● |
| Job submit (buyer) | ● | ○ | ● | ● |
| Non-custodial keys | ● | ● | session / connect | policy-based |
| GRID → BTC exit | ● | ● | via connect | batch / payroll |
| Multi-user / SSO | — | — | ○ | ● |
| Fleet + audit | — | — | ○ | ● |

● full · ○ partial · — not primary

### Delivery sequence

1. **P1 (now)** — CLI `grid wallet` stub; earn on coordinator; TSL messaging  
2. **P2** — Desktop **software wallet** (Tauri or native) calling local `grid`; web **buyer** portal  
3. **P3** — Genesis Earn balances in wallet; mobile **watch + alerts**; service **API + API keys**  
4. **P4** — Mobile signing + BTC/Lightning cash-out; web WalletConnect-class; fleet payroll to BTC  

### Security notes (edge)

- Never ship seed phrases through the coordinator API  
- Separate **node operator key** from **treasury / cash-out key** when possible  
- Services tier: explicit custody mode + audit; default remains non-custodial  
- Bitcoin TSL: prefer on-device or user-wallet BTC addresses for exits  

Implementation tracking lives in [technical.md](./technical.md); token rules in [GRID_Token_Specification.md](./GRID_Token_Specification.md).

---

## Docs

| File | Role |
|------|------|
| [technical.md](./technical.md) | Build plan, cost, phases |
| [GRID_White_Paper.md](./GRID_White_Paper.md) | Vision |
| [GRID_Token_Specification.md](./GRID_Token_Specification.md) | Token + Bitcoin TSL + Genesis Earn |
| [letter.md](./letter.md) | Letter to miners |
| [OnePage.pdf](./OnePage.pdf) | Pitch |
| [scripts/install.sh](./scripts/install.sh) | `curl \| bash` installer |

## Layout

```
src/                 Rust grid CLI + coordinator
scripts/install.sh   curl installer
legacy/ts/           historical TS MVP (optional)
contracts/           future on-rail stubs
```

## License

MIT (code). Docs CC-BY-4.0 where noted.

---

```bash
curl -fsSL https://raw.githubusercontent.com/Caraveo/grid/master/scripts/install.sh | bash
grid coord &
grid node
grid submit --wait
```

*No public testnet economy. Mainnet path. Bitcoin secures the exit.*
