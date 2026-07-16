![GRID](./GRID.jpg)

# GRID

**Useful mining** for a planetary compute network.  
Run a node. Do real work. Earn **GRID**. Cash out toward **Bitcoin**.

**Bitcoin is the Transact Security Layer** — GRID meters compute; BTC secures value.

> **Mainnet path only.** There is **no public testnet economy**.  
> Software can be staged privately; the token economy is **Genesis Earn → open utility** on mainnet rails (see [technical.md](./technical.md) and the [token specification](./GRID_Token_Specification.md)).

---

## Install

### One-liner (`curl`)

```bash
curl -fsSL https://raw.githubusercontent.com/Caraveo/grid/master/scripts/install.sh | bash
```

Force reinstall or always build from source:

```bash
curl -fsSL https://raw.githubusercontent.com/Caraveo/grid/master/scripts/install.sh | bash -s -- --force
curl -fsSL https://raw.githubusercontent.com/Caraveo/grid/master/scripts/install.sh | bash -s -- --from-source
```

Custom install directory:

```bash
curl -fsSL https://raw.githubusercontent.com/Caraveo/grid/master/scripts/install.sh | bash -s -- --prefix="$HOME/bin"
```

The script:

1. Tries a **GitHub Release** prebuilt (`grid-<os>-<arch>`) when available  
2. Otherwise **clones + `cargo build --release`** and installs to `~/.local/bin`  
3. Prints next steps for `grid coord` / `grid node`

Ensure `~/.local/bin` is on your `PATH` if it isn’t already.

### From source (manual)

```bash
git clone https://github.com/Caraveo/grid.git
cd grid
cargo build --release
cp target/release/grid ~/.local/bin/grid   # or /usr/local/bin
grid --version
```

Requires [Rust](https://rustup.rs) (stable) and a C toolchain.

---

## Quick start (three terminals)

```bash
# 1) Coordinator
grid coord

# 2) Miner
grid init --name garage --class S   # once
grid node

# 3) Job
grid submit --job echo --payload "hello-grid" --wait
grid stats
```

| Command | What |
|---------|------|
| `grid coord` | Job coordinator |
| `grid node` | Miner — claim work, earn |
| `grid init` | Write `~/.grid/config.toml` |
| `grid submit` | Submit allowlisted job (`echo`, `hash_file`) |
| `grid stats` | Jobs + nodes |
| `grid status` | Config + host metrics + Bitcoin TSL |
| `grid wallet` | Balance stub + GRID → BTC exit reminder |
| `grid test` | Offline executor smoke test |
| `grid resources` | CPU / memory sample |

---

## What Phase 1 is

**In**

- Single **Rust** `grid` binary (coord + node + submit)  
- Allowlisted jobs, verify-by-recompute  
- PoR-weighted earn, whale γ, little-miner inclusion  
- Class **S / M / L** (home → rack → datacenter)  
- Bitcoin documented as **Transact Security Layer**

**Later**

- Docker / GPU kernels, Genesis Earn locks on-rail  
- P2P mesh, critical-latency / gaming fabric  
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
