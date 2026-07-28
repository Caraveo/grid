# GRID Technical Plan

**Status:** living engineering doc  
**Audience:** founder + builders  
**Companion docs:** `GRID_White_Paper.md`, `GRID_Token_Specification.md`, `letter.md`, `OnePage.pdf`, `docs-cli-architecture.md`  
**Code home:** Rust `grid` binary (merged from [Caraveo/grid](https://github.com/Caraveo/grid)) + optional `legacy/ts` coordinator

---

## 0. Honest bottom line

| Question | Answer |
| --- | --- |
| **Is there a codebase today?** | **Yes — Phase 1 Rust.** Single `grid` binary: `coord` + `node` + `submit`, PoR earn, Bitcoin TSL. `legacy/ts` is optional history. |
| **Can you build without going broke?** | **Yes — if you build a thin MVP first** and refuse to fund datacenters, custom L1s, or full “planetary fabric” on day one. |
| **How much time do you have?** | Clock is yours. Industry reality: **3–6 months** to a demo; **9–18 months** to Genesis Earn mainnet with real miners; **years** for ms-critical gaming fabric. |
| **Can Grok code this for you?** | **Yes, in slices.** Full GRID is a multi-year product. We scaffold and implement **MVP modules** you can run on a laptop + one GPU. |

**You do not go broke by:** writing software, using free/cheap L2 deploys, running your own node, and paying for audits only when real money is at stake.  
**You go broke by:** buying H100 clusters, hiring a 20-person team pre-product, custom blockchain, or seeding millions in liquidity before demand.

---

## 1. Time map (realistic)

Assume **1 strong full-stack founder** (you) + heavy AI-assisted coding, part-time legal later.

| Phase | Clock | What “done” means | Cash pressure |
| --- | --- | --- | --- |
| **P0 — Spec freeze + repo** | 1–2 weeks | This doc + monorepo + contracts stubs | ~\$0 |
| **P1 — Local fabric demo** | **Done (refactor)** | `grid coord` + `grid node` + allowlisted jobs + verify + PoR earn | ~\$0 |
| **P2 — Private pilot** | 3–6 months | 5–20 real GPUs, docker jobs, dashboard, PoR score v0 | ~\$0–2k |
| **P3 — Genesis Earn Year start** | 9–18 months | Token + locks + emission controller on L2; miners earn locked GRID for verified work | ~\$5k–40k (audit optional→required before public money) |
| **P4 — Open utility** | +6–12 months after P3 | Buyers spend GRID/USDC for capacity; marketplace exit | Liquidity + compliance (this is where capital helps) |
| **P5 — Critical latency / gaming** | 2–5+ years | Edge RTT routing, competitive session SLAs | Only after utility revenue |

**You are not late.** DePIN compute is still early. Your differentiator is story + miner inclusion + useful work — but **shipping a boring batch job runner beats a perfect white paper.**

### If you only have nights/weekends
Multiply calendar time by ~2–3. Prefer P1 demo in public before any token.

---

## 2. Can you build this without going broke?

### Yes path (recommended)

Spend almost nothing until software works:

| Item | Cheap approach | Avoid |
| --- | --- | --- |
| Compute supply | **Your GPU + friends’ rigs** | Renting a cluster |
| Coordinator | Single VPS (\$5–20/mo) or home server | Multi-region K8s day one |
| Chain | **Base (or similar L2)** + existing tooling | Custom L1 / appchain |
| DEX / cash-out | External Uniswap-style pool later | Building your own exchange |
| Verification v0 | Re-execution on 2nd node + hash | Full ZK proving stack |
| Node runtime | Docker + NVIDIA container toolkit | Custom hypervisor |
| Legal | Bootstrap as software; delay public token sale | Raising via unregistered offering without counsel |

### Rough cash bands (USD, order-of-magnitude)

| Stage | Solo lean | “Serious but lean” |
| --- | --- | --- |
| Demo (P1) | \$0–500 | \$1–3k |
| Pilot (P2) | \$500–3k | \$5–15k |
| Pre-public contracts + light audit | \$0 (unaudited, dangerous) | \$15–80k audit |
| Genesis Earn public | software free; **reputation risk** if unaudited | \$30–150k+ (audit, ops, small liquidity) |
| Competing with AWS | **N/A — don’t** | Don’t |

**Token liquidity is optional for Genesis Earn** if tokens are locked and not marketed as investments. When you open cash-out and buyer markets, you need either organic volume or seed liquidity — that *can* get expensive; delay it.

### What “broke” usually means here
1. Premature token launch + market making  
2. Paying for supply (renting GPUs) instead of incentivizing owners  
3. Building all four white-paper layers before one workload works  

GRID’s own thesis saves you money: **miners bring hardware; you bring coordination software.**

---

## 3. MVP definition (what we actually build first)

**MVP name:** `grid-mvp` — *Verified container jobs + PoR score + earn ledger*

### In scope
1. **Node agent** — register, heartbeat, pull job, run **allowlisted Docker image**, return result + metrics  
2. **Coordinator** — job queue, schedule to one node, store results  
3. **Verifier v0** — second pass or checksum policy; reputation up/down  
4. **PoR score v0** — work units + uptime + success rate (weights from token spec)  
5. **Earn ledger** — off-chain first, then mirror to contracts in P3  
6. **Operator dashboard** — CLI or simple web: balance, jobs, status  
7. **Buyer CLI** — submit job, pay later (P2 pilot can be invite-only)

### Out of scope for MVP (spec stays aspirational)
- Planetary multi-continent fabric  
- Competitive P2P gaming ms fabric (Phase D)  
- Full AMM marketplace  
- ZK proofs for arbitrary ML  
- Custom chain  
- Training 70B models across home GPUs as launch claim  

### First workload (wedge)
**Allowlisted batch jobs only**, e.g.:
- Image/video transform container  
- Small ONNX/TensorRT inference container  
- Blender/frame-style offline render later  

One image family → easy sandboxing and verification.

---

## 4. Target architecture (implementable)

### 4.0 Bitcoin = Transact Security Layer (TSL)

**Design decision:** Bitcoin is the **Transact Security Layer** in GRID’s implementation.

| Layer | What it does | What it is *not* |
| --- | --- | --- |
| **Work** | PoR, jobs, nodes (`grid node`) | Money |
| **Utility** | GRID token meters compute, emissions, job escrow | Hard money / final settlement |
| **Fast rails** (optional) | L2 / Lightning for cheap high-frequency ops | Source of security |
| **Transact Security Layer** | **Bitcoin** — cash-out, high-value finality, commitment anchoring | Compute metering |

**Implications for build order:**
1. Earn GRID for verified work (utility ledger).  
2. Exit path: **GRID → BTC** first-class (USDC only as fiat leg).  
3. Later: anchor epoch roots / high-value bond events to Bitcoin (timestamp / commitment).  
4. Never invent a “better Bitcoin” inside GRID—**use Bitcoin**.

```
┌─────────────┐     ┌──────────────────┐     ┌──────────────┐
│ Buyer CLI   │────▶│ Coordinator API  │────▶│ Job Queue    │
└─────────────┘     │  (Rust / TS)     │     └──────┬───────┘
                    └────────┬─────────┘            │
                             │                      ▼
                    ┌────────▼─────────┐     ┌──────────────┐
                    │ Verifier / PoR   │◀────│ grid node    │
                    └────────┬─────────┘     │ (Rust)       │
                             │               └──────────────┘
                    ┌────────▼─────────┐
                    │ GRID utility     │  meter + earn
                    │ ledger / rail    │
                    └────────┬─────────┘
                             │ cash-out / finality
                    ┌────────▼─────────┐
                    │ Bitcoin TSL      │  Transact Security Layer
                    └──────────────────┘
```

### Suggested stack (cheap, hireable, AI-friendly)

| Layer | Choice | Why |
| --- | --- | --- |
| Node agent | **Rust `grid` binary** (Caraveo/grid merge) | One binary for miners |
| Coordinator | **TypeScript** in `legacy/ts` (MVP); Rust later | Fast job API iteration |
| DB | **Postgres** (SQLite for laptop demo) | Boring and solid |
| Queue | Postgres + `FOR UPDATE SKIP LOCKED` or Redis | Avoid Kafka early |
| Containers | Docker + NVIDIA runtime | Miners already understand this |
| Chain (P3) | Standard SPL token + audited Solana programs | Low fees, mature token and wallet ecosystem |
| Frontend | Next.js minimal or CLI-only first | Don’t burn months on UI |

*Stack is a recommendation, not religion. Consistency > perfection.*

### Token contracts (P3 modules only)

1. `GRIDToken` — standard Solana SPL token, 9 decimals, no freeze authority
2. `GRIDEmissionController` — capped mint authority, epoch budget, receipt replay protection
3. `GenesisLock` — earn-year vesting  
4. `NodeRegistry` — bonds, class, cluster id  
5. `JobEscrow` — post–Genesis Earn only  

Do **not** ship JobEscrow spend path before Genesis locks are clear.

---

## 5. Spec changes for buildability (adopted)

These reconcile white-paper ambition with survival:

| Spec idea | Engineering rule |
| --- | --- |
| Planetary fabric | **Start single-region coordinator**; multi-region later |
| Proof-of-Resource full vector | **v0 = completed work + uptime + success**; efficiency later |
| Verification | **v0 redundant/challenge**; not full crypto proof of ML |
| 1-hour epochs | OK; ledger can batch writes |
| Whale \(\gamma=5\%\) | Enforce off-chain first; on-chain in EmissionController. **Dynamic floor:** effective \(\gamma = \max(0.05, 1/N)\) so early networks with few clusters still distribute the full epoch pool; large \(N\) restores the 5% hyperscaler ceiling. |
| Phase D gaming | **Roadmap only** until batch utility works |
| Marketplace | Use **external DEX** when unlocking; don’t build CEX |
| Mainnet no testnet | Private pilot = software staging; **economy** starts at Genesis Earn |

Full token math remains in `GRID_Token_Specification.md`. This file is the **build filter**.

---

## 6. Security & “don’t get rekt” checklist

- Sandbox: only allowlisted images; no arbitrary host mounts  
- Resource limits: CPU/GPU/memory caps from operator config  
- No root-required miner UX if avoidable  
- Keys: operator wallet never on coordinator  
- Before any public mint: **audit EmissionController + Token + Lock**  
- Legal: utility narrative ≠ free pass; get counsel before public distribution  

---

## 7. Cost of *not* owning compute (your advantage)

| Approach | Capital | GRID thesis |
| --- | --- | --- |
| Buy/rent GPU farm | Very high | **Reject for bootstrap** |
| Pay users fiat to run nodes | Medium | Optional tiny bounties only |
| Emit locked GRID for PoR | Low software cost | **Primary** |
| Enterprise paid pilots (USDC) | Revenue | Fund ops without selling soul |

If Google joins later: they take **job revenue** under caps; they do **not** mint unlimited GRID (`γ` ceiling + fixed \(M_t\)). Details in token spec.

---

## 8. Edge wallets (summary)

Full surface plan lives in **[README.md § Edge wallets](./README.md#edge-wallets-roadmap)**.

| Surface | Intent |
| --- | --- |
| **Software** | Desktop non-custodial wallet + local `grid` control |
| **Mobile** | Watch / alerts → later sign + BTC exit |
| **Web** | Buyer portal + wallet connect |
| **Services** | Fleet API, payroll to BTC, optional custody policies |

**Constraint:** Solana devnet is a non-economic engineering environment only;
devnet GRID has no monetary value. Wallets must label it clearly and must not
present it as Genesis Earn/mainnet value. **Bitcoin = TSL** for cash-out.

## 9. What to implement next

Ordered by leverage:

1. Publish GitHub Release prebuilts so `curl | bash` skips compile  
2. Desktop software wallet shell (P2) wrapping `grid`  
3. Genesis Earn locks on-rail + wallet balance UI  
4. Mobile watch app + service API keys  
5. Docker/GPU job kinds  

**Not a weekend:** full marketplace, gaming fabric, production multi-tenant security.

---

## 9. Decision checklist (founder)

Answer these before spending money:

- [ ] Will the first job be a **single allowlisted container**? (should be yes)  
- [ ] Can you run the first 10 nodes from **people you know**?  
- [ ] Is token launch **blocked** until earn + lock work in software?  
- [ ] Is gaming/ms marketing **story only** until P4+?  
- [ ] Is “without going broke” defined as **&lt; \$X** personal risk? Write the number.

**Suggested personal risk cap before revenue:** \$1k–5k if solo and careful; pause if over.

---

## 10. Repository layout (target)

```
GRID/
  technical.md                 ← this file
  GRID_White_Paper.md
  GRID_Token_Specification.md
  letter.md
  OnePage.pdf
  apps/
    coordinator/               ← API + scheduler
    cli/                       ← `grid` binary: grid node | submit | stats
    node/                      ← thin wrapper → grid node
  contracts/                   ← Solidity (P3)
  packages/
    por/                       ← scoring shared lib
    protocol/                  ← job schema, hashes
  deploy/
    docker-compose.yml         ← local demo
  docs/
    miner-quickstart.md
```

---

## 11. Success metrics (not vanity)

| Stage | Metric |
| --- | --- |
| P1 | 1 job, 1 node, verified result, replayable demo |
| P2 | ≥10 nodes, ≥100 jobs/week, &lt;5% unexplained fail |
| P3 | Emission matches schedule; whale cap enforced; locks work |
| P4 | Buyer pays; node unlocks and sells without manual rescue |

---

## 12. Final answer to “can I?”

**Yes. You can build the first real GRID without going broke** by treating the white paper as the north star and this file as the runway:

1. Software first  
2. Friends’ GPUs second  
3. Locked earn third  
4. Open market fourth  
5. Planetary + gaming last  

The supercomputer is a mesh. **Your first mesh can be three machines and a VPS.**

— technical plan v1.0
