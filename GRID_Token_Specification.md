<urp version="0.1">
  <paper>
    <id>urp:grid:token-spec:1.0</id>
    <title>GRID Token Specification</title>
    <subtitle>Proof-of-Resource Economy, Marketplace Settlement, and Growth Design</subtitle>
    <type>white-paper</type>
    <language>en-US</language>
    <version>1.5.0</version>
    <created>2026-07-16</created>
    <updated>2026-07-16</updated>
    <status>draft</status>
  </paper>
  <authors>
    <author>
      <name>Jon Caraveo</name>
      <affiliation>GRID Network</affiliation>
      <orcid>0000-0002-1825-0097</orcid>
    </author>
  </authors>
  <license>
    <text>CC-BY-4.0</text>
    <code>MIT</code>
    <data>CC-BY-4.0</data>
  </license>
  <reproducibility>
    <level>0</level>
  </reproducibility>
</urp>

# Abstract

GRID is a planetary-scale distributed compute fabric secured by Proof-of-Resource (PoR): verified useful work performed by autonomous nodes rather than energy spent on non-productive puzzles. This paper specifies the native utility token **GRID**, the economic layer that meters capacity, settles verified jobs, and rewards hardware operators.

**Bitcoin is the Transact Security Layer** in the GRID implementation. GRID meters *compute*; Bitcoin secures *value in transit and at rest* when operators cash out, when high-value settlements finalize, and when the network needs the hardest monetary settlement substrate on Earth. GRID does not replace Bitcoin as money. It plugs useful mining into an economy that can exit into—and settle on—Bitcoin.

A defining economic consequence of PoR is **hardware continuity for miners**. Operators who already own working GPUs—including former Ethereum GPU miners after the network’s move away from Proof-of-Work, and Bitcoin-era operators who still control GPU fleets—can keep those machines productive by “mining” verified useful compute on GRID instead of retiring iron or chasing only puzzle-based hash rate. Datacenters are welcome as first-class supply, but the protocol is deliberately designed so **little miners** (home rigs, single GPUs, small workshops) remain competitive participants, not decorative leftovers.

Looking forward, GRID is not only a batch supercomputer. Future use cases include **latency-critical** applications—competitive peer-to-peer gaming measured in milliseconds, live cloud play, spatial multiplayer, edge AI for interactive worlds—where geography and edge nodes matter as much as raw FLOPs. In those regimes, the little miner near the player is not a compromise; it is the product.

We define token supply, emission, allocation, escrow settlement, reputation-weighted rewards, demand-side sinks, marketplace design, inclusive scheduling, critical-latency routing assumptions, and a staged growth program that bootstraps both compute supply and buyer demand without requiring the protocol to own the global hardware fleet. A companion plain-language invitation to operators—**A Letter to Miners**—is published separately as `letter.md`. Parameters herein are **explicit design assumptions** chosen so that technical coordination and network growth can function end-to-end; they are intended to be revised through governance after live network measurement.

# Problem

## Capital and Compute Concentration

Modern AI, graphics, simulation, and spatial computing workloads require vast GPU and CPU capacity. Capital for that capacity is concentrated in hyperscale data centers and a small set of cloud providers. Independent operators—developers, labs, studios, and individuals—face two coupled barriers:

1. **Access to capital** to rent or purchase accelerators at competitive scale.
2. **Access to elastic compute** that can grow without multi-year procurement cycles.

GRID’s base white paper describes how heterogeneous machines form a unified fabric. The unresolved economic problem is coordination: strangers will not contribute reliable hardware unless contribution is measured, paid, and liquid. Without a credible reward and cash-out path, idle capacity remains idle.

## Why Abstract Mining Fails Here

Bitcoin demonstrated that cryptographic incentives can organize global participation. Proof-of-Work, however, optimizes for puzzle hardness, not for training models, rendering frames, or serving inference. A compute network needs the opposite property: **rewards must track verified useful output**. Capital should flow toward machines that complete real work with fidelity, not toward pure hash rate.

## The Miner Hardware Continuity Problem

Mining communities already built the world’s most battle-tested decentralized hardware culture: multi-GPU rigs, power management, uptime discipline, thermal control, and 24/7 operations. Two historical shifts stranded enormous **working** capacity:

1. **Ethereum’s exit from GPU mining** left vast fleets of consumer and prosumer GPUs without a native PoW home—machines that still excel at parallel math, rendering, and AI kernels.
2. **Bitcoin’s industrialization toward ASICs** made pure hash mining uneconomic for many GPU-heavy operators, even when those GPUs remained perfectly capable of useful parallel workloads.

GRID treats this not as scrap metal, but as **ready-made planetary supply**. The spectacular property of Proof-of-Resource is continuity of vocation: **miners can keep mining**—same rooms, same power drops, same operational habits—while the product of mining becomes verified capacity for real applications. The token and marketplace then turn that work into GRID that can be sold for fiat or other crypto, restoring the economic loop miners already understand.

This is outstanding for the network as well as for miners: bootstrap does not require manufacturing a new global hardware base from zero. It re-aims hardware that already exists.

## Inclusive Scale: Datacenters and Little Miners Together

If only hyperscale datacenters can earn, GRID collapses into a slightly more complicated cloud. If only hobby nodes can earn, enterprise buyers will not trust capacity. The economic design must therefore support **both ends of the spectrum at once**:

| Operator class | Example hardware | Role on GRID |
| --- | --- | --- |
| Little miner | 1–4 consumer GPUs, home/workshop | Edge, batch shards, elastic overflow, geographic diversity |
| Mid fleet | Former mining racks, small colos | Steady parallel jobs, rendering, inference pools |
| Datacenter / enterprise | Dense GPU servers, SLAs | Large contiguous jobs, high-memory training slices, guaranteed capacity |

The design goal is not “small only” or “big only.” It is **permissionless participation with anti-exclusion rules**: datacenters increase total \(Q_t\) (good for buyers), while scheduling, bonding, and reward policy keep little miners economically alive (good for decentralization, latency diversity, and resilience).

## The Two-Sided Bootstrap

Any compute marketplace is a two-sided market:

| Side | Actor | Needs |
| --- | --- | --- |
| Supply | Node operators (miners of resource) | Predictable rewards and liquid exit to fiat or crypto |
| Demand | Developers, labs, apps | Reliable capacity, clear pricing, simple APIs |
| Settlement | Protocol + marketplace | Trustless metering, low fees, auditable payouts |

If only supply exists, tokens lack real demand. If only demand exists, buyers cannot find capacity. If rewards cannot be sold for USD, BTC, ETH, or stablecoins, most operators will not stay online. The token specification must therefore cover **work metering**, **payment**, and **marketplace liquidity** as one system—not as optional product features.

# Methodology

## Design Goals

The GRID token economy is designed to satisfy eight goals simultaneously:

1. **Utility first** — GRID is required to purchase network capacity and to settle verified jobs.
2. **Useful mining** — emissions reward Proof-of-Resource, not abstract puzzles; former GPU miners keep hardware productive.
3. **Liquid exit** — earned tokens must be sellable for fiat or other cryptocurrencies via marketplace pairs.
4. **Demand sinks** — consumption, partial burn, and escrow create structural demand for GRID.
5. **Sybil resistance** — reputation, staking bonds, and verification prevent fake capacity.
6. **Bootstrap without owning hardware** — protocol capital funds software, liquidity, and early incentives—not a proprietary supercluster.
7. **Hardware rebirth** — working GPUs from the Ethereum/Bitcoin mining eras remain first-class economic citizens.
8. **Little-miner inclusion** — datacenter join is welcome; protocol rules prevent large operators from monopolizing eligibility so home and small fleets still receive work and rewards.

## Measurement Model (Proof-of-Resource)

Each epoch \(t\), node \(i\) produces a resource score \(R_{i,t}\) from verified telemetry and job outcomes:

$$
R_{i,t} = w_c C_{i,t} + w_u U_{i,t} + w_e E_{i,t} + w_f F_{i,t}
$$
{#eq-resource-score title="Per-Epoch Resource Score"}

<variables for="eq-resource-score">
  <variable symbol="$R_{i,t}$">composite resource score of node $i$ in epoch $t$</variable>
  <variable symbol="$C_{i,t}$">normalized verified compute work units completed</variable>
  <variable symbol="$U_{i,t}$">normalized availability / uptime score</variable>
  <variable symbol="$E_{i,t}$">normalized energy-efficiency score (work per joule proxy)</variable>
  <variable symbol="$F_{i,t}$">normalized fidelity score (accuracy, rework rate, challenge pass rate)</variable>
  <variable symbol="$w_c,w_u,w_e,w_f$">non-negative weights with $w_c + w_u + w_e + w_f = 1$</variable>
</variables>

**Assumed default weights (mainnet v1):** \(w_c = 0.55\), \(w_u = 0.15\), \(w_e = 0.10\), \(w_f = 0.20\). Weights may be adjusted by governance after measuring fraud rates and workload mix.

A reputation multiplier \(\rho_{i,t} \in [0.5, 1.5]\) modulates score based on rolling history:

$$
S_{i,t} = R_{i,t} \cdot \rho_{i,t}
$$
{#eq-effective-score title="Reputation-Weighted Effective Score"}

<variables for="eq-effective-score">
  <variable symbol="$S_{i,t}$">effective score used for reward allocation</variable>
  <variable symbol="$\rho_{i,t}$">reputation multiplier for node $i$ at epoch $t$</variable>
</variables>

## Emission and Reward Allocation

Let \(M_t\) be the protocol emission in epoch \(t\) (GRID units). To protect little miners without excluding datacenters, emission is split into a **proportional pool** and an **inclusion pool**:

$$
M_t = M_t^{\text{prop}} + M_t^{\text{inc}}
$$
{#eq-emission-split title="Proportional and Inclusion Emission Pools"}

<variables for="eq-emission-split">
  <variable symbol="$M_t$">total GRID emission in epoch $t$</variable>
  <variable symbol="$M_t^{\text{prop}}$">score-proportional pool (assumed $0.90 M_t$)</variable>
  <variable symbol="$M_t^{\text{inc}}$">little-miner inclusion pool (assumed $0.10 M_t$)</variable>
</variables>

The proportional pool pays pure performance:

$$
r_{i,t}^{\text{prop}} = M_t^{\text{prop}} \cdot \frac{S_{i,t}}{\sum_{j} S_{j,t}}
$$
{#eq-node-reward title="Proportional Emission Reward"}

<variables for="eq-node-reward">
  <variable symbol="$r_{i,t}^{\text{prop}}$">GRID emission from the proportional pool paid to node $i$ for epoch $t$</variable>
  <variable symbol="$M_t^{\text{prop}}$">proportional emission in epoch $t$</variable>
</variables>

The inclusion pool is reserved for **eligible small-class nodes** (assumed: advertised capacity below a class threshold \(C_{\text{small}}\), e.g. up to 4 consumer GPUs or equivalent work-unit rating) that completed at least one verified job or heartbeat window in the epoch:

$$
r_{i,t}^{\text{inc}} = M_t^{\text{inc}} \cdot \frac{\mathbf{1}_{i \in \mathcal{L}_t} \cdot S_{i,t}}{\sum_{j \in \mathcal{L}_t} S_{j,t}}
$$
{#eq-inclusion-reward title="Little-Miner Inclusion Reward"}

<variables for="eq-inclusion-reward">
  <variable symbol="$r_{i,t}^{\text{inc}}$">GRID emission from the inclusion pool paid to small-class node $i$</variable>
  <variable symbol="$\mathcal{L}_t$">set of little-miner nodes active and verified in epoch $t$</variable>
  <variable symbol="$\mathbf{1}_{i \in \mathcal{L}_t}$">indicator that node $i$ is in the little-miner set</variable>
</variables>

Total node emission: \(r_{i,t} = r_{i,t}^{\text{prop}} + r_{i,t}^{\text{inc}}\). Datacenters still dominate raw proportional rewards when they contribute more verified work (fair to scale), but they **cannot drain** the inclusion pool—so a healthy home-miner cohort keeps a dedicated claim on bootstrap and long-run emissions.

**Assumption:** epoch length is **1 hour** on mainnet v1 (fast feedback for operators; batchable for L2 gas efficiency).

## Job Settlement Escrow

When a buyer submits a job with quoted cost \(P\) GRID:

1. Buyer locks \(P\) in an escrow contract.
2. Coordination layer assigns shards to eligible nodes.
3. Verification layer accepts or rejects results.
4. On success, escrow releases according to the settlement split in @eq-settlement-split.
5. On failure, escrow refunds residual to buyer after slashing any bonded misbehavior.

$$
P = P_{\text{nodes}} + P_{\text{burn}} + P_{\text{treasury}}
$$
{#eq-settlement-split title="Job Settlement Split"}

<variables for="eq-settlement-split">
  <variable symbol="$P$">total job price locked in escrow (GRID)</variable>
  <variable symbol="$P_{\text{nodes}}$">share paid to verified node operators</variable>
  <variable symbol="$P_{\text{burn}}$">share permanently removed from circulating supply</variable>
  <variable symbol="$P_{\text{treasury}}$">share to protocol treasury for security, R&amp;D, and growth</variable>
</variables>

**Assumed mainnet v1 split:** \(P_{\text{nodes}} = 0.85P\), \(P_{\text{burn}} = 0.10P\), \(P_{\text{treasury}} = 0.05P\).

## Dynamic Capacity Pricing

Let \(D_t\) be demand (quoted work units) and \(Q_t\) be available verified capacity in the same units. A simple market clearing price for the next window is:

$$
\pi_t = \pi_0 \cdot \left(\frac{D_t + \epsilon}{Q_t + \epsilon}\right)^{\alpha}
$$
{#eq-dynamic-price title="Dynamic Capacity Price"}

<variables for="eq-dynamic-price">
  <variable symbol="$\pi_t$">GRID price per standardized work unit in window $t$</variable>
  <variable symbol="$\pi_0$">baseline price parameter</variable>
  <variable symbol="$D_t$">aggregate demand in work units</variable>
  <variable symbol="$Q_t$">aggregate verified supply in work units</variable>
  <variable symbol="$\epsilon$">small constant to avoid division by zero</variable>
  <variable symbol="$\alpha$">price elasticity exponent (assumed $\alpha = 0.7$)</variable>
</variables>

When \(\pi_t\) rises, node rewards become more attractive in fiat terms (via marketplace), pulling supply online—the same self-balancing logic described in the GRID white paper’s resource economy.

## Growth Methodology (Assumptions)

We assume a **three-phase cold start** on **mainnet from day one** (no separate public testnet economy):

1. **Genesis Earn Year (months 0–12):** miners earn GRID for verified useful work under a hard epoch emission ceiling, but **earned tokens are not freely spendable for network capacity** during this year—they accrue under a time-lock / vesting unlock (assumed linear unlock over months 12–18, or cliff at month 12). Work is **not free** for buyers either: foundation and early partners may run paid pilot jobs in stablecoin or locked GRID, while open token-for-capacity markets fully open after Genesis. Goal: measure hardware, prove PoR, distribute ownership to operators **without** flooding a hot spend market or letting a hyperscaler drain the float.
2. **Open utility (months 12–36):** capacity market live; one primary workload wedge (assumed: **GPU inference + batch AI jobs**, then rendering) drives organic token demand; emissions taper.
3. **Self-sustaining market (year 3+):** majority of operator income comes from job settlement \(P_{\text{nodes}}\), not pure emission \(M_t\).

This sequence is deliberate: Bitcoin bootstrapped security via emission; GRID bootstraps **useful capacity** via a disciplined earn year, then migrates value capture to real compute demand.

# Architecture

## Layered Economic Stack

```arcmark
nodes:
  work: "Work Layer — PoR / jobs / nodes"
  util: "Utility Layer — GRID token (meter compute)"
  rails: "Optional fast rails — L2 / Lightning"
  btc: "Transact Security Layer — Bitcoin"
links:
  work -> util
  util -> rails
  util -> btc
  rails -> btc
```
{#diag-btc-tsl title="Bitcoin as Transact Security Layer"}

| Layer | Asset / system | Role |
| --- | --- | --- |
| **Work** | Proof-of-Resource, node fabric | Measure and verify useful compute |
| **Utility** | **GRID** token | Price capacity, emit rewards, job escrow, inclusion economics |
| **Fast rails** (optional) | L2 contracts, Lightning | Cheap high-frequency ops while UX stays snappy |
| **Transact Security Layer** | **Bitcoin** | Hard settlement, operator cash-out, high-value finality, commitment anchoring |

**Implementation principle:** anything that must remain true about *money movement* ultimately rests on Bitcoin’s security model. GRID is not a competing monetary base; it is compute-market infrastructure that **settles value into Bitcoin** (and optionally fiat via BTC ramps).

### Bitcoin Transact Security Layer (assumptions)

1. **Cash-out preference** — marketplace pairs prioritize **GRID → BTC** (and GRID → USDC for fiat legs). BTC is first-class, not an afterthought.  
2. **High-value finality** — large treasury moves, bond slashes above a threshold, and epoch emission roots may be **anchored** to Bitcoin (e.g. commitment hash / timestamp) so the ledger of who earned what is cryptographically tied to the TSL.  
3. **Operator mental model** — miners who already hold BTC keep one stack: earn GRID for work → sell or swap toward BTC for long-term savings.  
4. **No BTC re-mint as GRID** — Bitcoin is not wrapped into emission accounting; GRID supply rules stay independent. BTC secures *transactions of value*, not *print of utility credits*.  
5. **Lightning optional** — when volume justifies it, Lightning can carry small BTC settlements; base-layer Bitcoin remains the security backstop.

## Token Profile (Assumptions)

| Parameter | Value (v1 assumption) |
| --- | --- |
| Name | GRID |
| Symbol | GRID |
| Standard | ERC-20 (and bridged equivalents) **or** equivalent on chosen utility rail |
| Decimals | 18 |
| Max supply | \(10 \times 10^9\) GRID (10 billion, hard capped) |
| **Transact Security Layer** | **Bitcoin** |
| Utility / metering rail | Fast L2 or app-chain for job escrow (implementation choice; not the TSL) |
| Native gas on utility rail | Chain gas (not GRID); GRID meters **compute**, not blockspace |
| Primary quote / exit | **BTC** first; USDC for fiat off-ramps |

GRID is a **utility and compute-settlement token**, not equity and not a claim on protocol legal ownership. **Bitcoin is money-security for transactions.** Legal classification is jurisdiction-dependent and out of scope for technical design; parameters assume a utility-first usage model for GRID and Bitcoin as the TSL.

## System Components

```arcmark
nodes:
  buyer: "Buyer / App (locks GRID for capacity)"
  coord: "Coordination Layer (schedule & route)"
  nodes: "GRID Nodes (execute workloads)"
  verify: "Verification Layer (PoR + challenges)"
  escrow: "Escrow & Settlement Contracts"
  emit: "Emission Controller"
  market: "Token Marketplace (GRID pairs)"
  off: "Fiat / Crypto Off-Ramps"
links:
  buyer -> escrow
  escrow -> coord
  coord -> nodes
  nodes -> verify
  verify -> escrow
  emit -> nodes
  escrow -> nodes
  escrow -> market
  nodes -> market
  market -> off
```
{#diag-token-flow title="GRID Token Economic Flow"}

### On-Chain Modules

1. **GRID Token Contract** — fixed max supply mint authority restricted to Emission Controller + genesis allocator.
2. **Emission Controller** — mints epoch rewards according to the schedule in Research; never exceeds remaining emission budget.
3. **Node Registry** — binds hardware identity commitments, operator wallets, stake bonds, and reputation roots.
4. **Job Escrow** — locks buyer funds, releases on verification proofs, supports refunds and slashing.
5. **Marketplace Router** — optional protocol-owned interface to AMM pools (GRID/USDC, GRID/ETH) for best-price routing; operators may also use external CEX/DEX venues.
6. **Treasury Multisig / Governance** — holds treasury allocation; spends via published budgets.

### Off-Chain / Hybrid Modules

1. **Coordination service** — work splitting, placement, SLA tracking (results anchored on-chain).
2. **Verification workers** — redundant execution, spot checks, ZK or fraud-proof friendly attestations as the stack matures.
3. **Marketplace UI** — swap GRID → USDC/ETH; fiat off-ramp via licensed partners (operator never forced to use a single venue).

## Genesis Allocation (Assumptions)

Total max supply \(S_{\max} = 10^{10}\) GRID.

| Bucket | Share | Amount (GRID) | Unlock / Policy |
| --- | --- | --- | --- |
| Network emissions (PoR rewards) | 45% | 4.5B | 10-year decaying schedule |
| Ecosystem & grants | 15% | 1.5B | Programmatic + milestone grants |
| Protocol treasury | 12% | 1.2B | Governance-controlled; ops, security, audits |
| Core contributors | 15% | 1.5B | 1-year cliff, 4-year linear vest |
| Liquidity & market making | 8% | 0.8B | Bootstrap AMM/CEX depth; transparency reports |
| Community bootstrap | 5% | 0.5B | Early node incentives, launch programs, targeted airdrops |

```arcmark
nodes:
  emit: "Emissions 45%"
  eco: "Ecosystem 15%"
  treas: "Treasury 12%"
  core: "Contributors 15%"
  liq: "Liquidity 8%"
  comm: "Community 5%"
links:
  emit -> emit
  eco -> eco
  treas -> treas
  core -> core
  liq -> liq
  comm -> comm
```
{#diag-allocation title="Genesis Allocation Buckets"}

**Rationale:** Nearly half of supply is reserved for people who actually run machines—the Bitcoin-like incentive core—while contributor and treasury shares are large enough to fund multi-year software without forcing the foundation to own the compute fleet.

## Emission Schedule (Assumptions)

Annual emission as a fraction of the 4.5B emission bucket decays approximately geometrically:

$$
M_{\text{year}}(y) = B \cdot \frac{(1 - \lambda)\lambda^{y}}{1 - \lambda^{Y}}
$$
{#eq-yearly-emission title="Yearly Emission from Reward Bucket"}

<variables for="eq-yearly-emission">
  <variable symbol="$M_{\text{year}}(y)$">GRID emitted in year $y$ from the rewards bucket</variable>
  <variable symbol="$B$">reward bucket size ($4.5 \times 10^9$)</variable>
  <variable symbol="$\lambda$">decay factor (assumed $0.82$)</variable>
  <variable symbol="$Y$">emission horizon in years (assumed $10$)</variable>
  <variable symbol="$y$">year index starting at $0$</variable>
</variables>

Illustrative yearly emissions (rounded):

| Year | Approx. emission (GRID) | Notes |
| --- | --- | --- |
| 0 | ~820M | Genesis Earn Year; earn under lock; high node APY in token terms |
| 1 | ~670M | Open utility; product wedge live |
| 2 | ~550M | Transition toward job-settlement income |
| 3–9 | decaying → residual | Majority of operator income from \(P_{\text{nodes}}\) |
| 10+ | 0 new emission | Pure utility circulation + burns |

Per-epoch emission is the yearly total divided by epochs per year (8760 for 1-hour epochs). The Emission Controller **cannot** mint above that epoch budget, above the remaining rewards-bucket balance, or above \(S_{\max}\). Extra work in a busy epoch dilutes per-node share of a **fixed** \(M_t\); it does not print unbounded GRID.

## Emission Safety and Hyperscaler Limits (Assumptions)

Disbursement is capped at multiple layers so “too many tokens” cannot be emitted just because a giant joins:

| Control | What it stops |
| --- | --- |
| **Hard max supply** \(S_{\max} = 10\text{B}\) | Infinite mint forever |
| **Rewards bucket** 4.5B over ~10 years | Emissions eating the whole supply |
| **Fixed epoch budget** \(M_t\) | Busy hours printing extra GRID |
| **Decaying yearly schedule** | Permanent hyper-emission |
| **Job burn** (10% of \(P\)) | Pure inflation with no sink when utility is live |
| **Anti-monopoly job cap** \(\kappa = 15\%\) | One operator taking all paid work while others idle |
| **Per-cluster emission ceiling** \(\gamma = 5\%\) of \(M_t^{\text{prop}}\) | One fleet (e.g. a hyperscaler) vacuuming most of an epoch’s mint |
| **Inclusion pool** 10% of \(M_t\) | Large operators draining little-miner rewards |
| **Genesis Earn Year locks** | Early float dumping into spend before the market is ready |

**Hyperscaler rule (assumed):** for any operator identity cluster \(G\) (including subsidiaries and co-located fleets under common control), proportional-pool rewards are capped:

$$
r_{G,t}^{\text{prop}} \le \gamma \cdot M_t^{\text{prop}}
$$
{#eq-whale-cap title="Per-Cluster Emission Ceiling"}

<variables for="eq-whale-cap">
  <variable symbol="$r_{G,t}^{\text{prop}}$">total proportional emission credited to operator cluster $G$ in epoch $t$</variable>
  <variable symbol="$\gamma$">maximum share of the proportional pool per cluster (assumed $0.05$)</variable>
  <variable symbol="$M_t^{\text{prop}}$">proportional emission pool in epoch $t$</variable>
</variables>

Overflow above the cap is redistributed to other eligible clusters in that epoch. **Engineering assumption (see `technical.md`):** effective ceiling is \(\gamma_{\text{eff}} = \max(\gamma, 1/N)\) where \(N\) is the number of active identity clusters, so a three-node network is not forced to leave 85% of the mint unissued, while a network with many clusters restores the 5% hyperscaler bound. Google-scale capacity is **welcome for jobs buyers pay for**; it is **not** entitled to mint the network’s entire emission schedule. Paid job revenue can still flow to big operators via escrow when they win verified work—subject to the 15% assignment cap when spare capacity exists elsewhere—while emission remains a public good with ceilings.

## Node Classes and Inclusive Scheduling

GRID nodes are hardware-agnostic by design (see the base GRID white paper). For economic fairness we assume three **capacity classes** used only for matching and policy—not for social status:

| Class | Assumed definition | Typical participant |
| --- | --- | --- |
| `S` (small) | \(\le C_{\text{small}}\) work-unit rating (e.g. 1–4 consumer GPUs) | Little miners, home labs |
| `M` (medium) | Between small and large thresholds | Former mining racks, small colo |
| `L` (large) | Datacenter-scale advertised capacity | Enterprise GPU clusters |

**Inclusive scheduling rules (v1 assumptions):**

1. **Shard-first packing** — large jobs are disassembled into micro-tasks whenever the workload allows; shards are eligible for `S`/`M` nodes, not only `L`.
2. **Class-matched queues** — buyers may request `any`, `prefer-edge`, or `prefer-dense`. Default `any` fills from the cheapest verified capacity across classes.
3. **Anti-monopoly cap** — no single operator identity cluster may claim more than \(\kappa\) of open shard assignments in a window (assumed \(\kappa = 15\%\)) while other eligible classes have idle capacity.
4. **Bond scales with class** — little miners are not priced out of registration (see staking).
5. **No rack minimum** — a single honest GPU with a wallet, bond, and node client is a full network citizen.
6. **Datacenter welcome** — `L` nodes receive contiguous multi-GPU jobs, premium SLAs, and proportional rewards without special political privilege beyond performance and reputation.

This is how datacenters and little miners coexist: big iron grows total supply and serves whale buyers; small iron remains economically reachable through sharding, caps, and the inclusion emission pool.

## Node Staking Bond and Slashing

To reduce fake capacity and griefing:

- Operators post a **stake bond** \(b_i\) in GRID (or accepted stablecoin converted and held as GRID) proportional to advertised capacity class.
- **Assumed bonds:** 250 GRID for class `S` (little miner), 1,000 GRID for class `M`, and scaled linear with advertised GPUs for class `L` (e.g. 1,000 GRID × GPU-equivalent units). Little miners stay capital-light.
- Proven fraud (fabricated results, withheld availability after acceptance, challenge failures beyond threshold) **slashes** a fraction of \(b_i\); slash is burned or sent to a security bounty pool (assumed 50/50).

Reputation \(\rho_{i,t}\) falls on slash events and recovers slowly with clean work—preferring reliable hardware over fly-by-night farms.

## Marketplace Architecture

The marketplace has two related venues:

### 1. Capacity Market (primary utility)

Buyers purchase **compute** priced in GRID (or auto-swapped from USDC at quote time). This is the economic heart of the network: tokens flow from demand to supply through escrow.

### 2. Token Market (secondary liquidity)

Node operators and other holders trade GRID against USDC/ETH (and later other crypto). This is the **cash-out surface**:

- Sell GRID → USDC → bank/fiat via on-ramp partners
- Sell GRID → ETH/BTC for crypto-native treasury management
- Buy GRID to fund upcoming job budgets

**Assumption:** launch includes a GRID/USDC AMM pool seeded from the Liquidity bucket with published inventory and a target initial depth sufficient for early operator exits without extreme slippage (exact USD seed is operational, not consensus-critical). External listings may follow once volume and compliance allow.

# Research

## Mapping to the GRID White Paper

Chapter 8 of the GRID white paper defines a tokenized marketplace where:

- Developers spend tokens to buy capacity.
- Nodes earn tokens via Proof-of-Resource.
- Settlement occurs after verification.
- Pricing rises under scarcity to attract supply.

This specification makes that chapter implementable by fixing (as assumptions) the token standard, supply, emission, settlement splits, reputation formula, marketplace dual structure, miner hardware continuity, and little-miner inclusion under datacenter participation.

**One-liner (emission discipline):** Hard caps, fixed epoch mints, whale emission ceilings, and a one-year Genesis Earn lock mean even a Google-scale join cannot print or drain unbounded GRID—big iron serves paid jobs; the mint stays on a schedule.

**One-liner (buildability):** Ship a single-region verified container fabric and locked earn ledger first; planetary routing and ms-critical gaming remain roadmap layers, not launch blockers—see `technical.md`.

**One-liner (Bitcoin TSL):** Bitcoin is the Transact Security Layer—GRID meters useful compute; BTC secures cash-out, high-value finality, and the hard settlement path operators already trust.

## Implementation Phasing (Engineering Assumptions)

To keep the protocol buildable without requiring founder-owned superclusters or unbounded capital:

1. **MVP** — allowlisted Docker jobs, coordinator queue, PoR v0 (work + uptime + success), off-chain earn ledger.  
2. **Genesis Earn** — L2 token + emission controller + vesting locks; no open capacity spend.  
3. **Open utility** — job escrow + external DEX cash-out; one workload wedge.  
4. **Scale** — multi-region, richer verification, latency classes, Phase D interactive.

Verification v0 assumes **redundancy / challenge**, not universal ZK for arbitrary models. Marketplace v0 uses **external AMM venues**, not a protocol-built exchange.

## Hardware Continuity: From Puzzle Mining to Useful Mining

GPU mining culture is not a footnote—it is GRID’s natural cold-start population.

| Legacy path | Hardware reality | GRID path |
| --- | --- | --- |
| Ethereum GPU mining (pre-Merge) | High-parallel GPUs, mining OS, PDUs, racks | Re-aim the same GPUs at verified AI/render/sim jobs; earn GRID |
| Bitcoin-era operators with GPUs | Mixed fleets; ASICs for hash, GPUs often idle or secondary | Keep ASICs on BTC if desired; point GPUs at GRID PoR “mining” |
| Idle gaming / workstation GPUs | Nighttime and workweek idle cycles | Schedule-limited node autonomy; earn while the machine would sit dark |
| Datacenter surplus | Reserved bursts, pre-launch clusters | Join as class `L`; sell spare capacity into the same marketplace |

The cultural pitch is intentional and precise: **you already know how to mine**. GRID changes *what* is proven—resource contribution and job fidelity—not the dignity of running machines for network reward. Operators sell earned GRID for USDC, ETH, BTC, or fiat exactly as they already manage crypto treasuries.

## Coexistence Research: Can Little Miners Survive Datacenters?

A naive proportional market would route most jobs and emissions to the largest fleets. That outcome is unacceptable for GRID’s decentralization thesis. The assumed countermeasures are mechanical, not rhetorical:

1. **10% inclusion emission pool** reserved for active class `S` nodes (@eq-inclusion-reward).
2. **Job sharding** so “one big training job” becomes thousands of little-miner-eligible units when architecture allows.
3. **15% anti-monopoly assignment cap** per operator cluster while spare capacity exists elsewhere.
4. **Progressive bonding** so home miners are not blocked by datacenter-scale stake requirements.
5. **Buyer defaults that do not hard-require** `L`-only capacity unless the workload truly needs it (e.g. multi-GPU NVLink domains).
6. **Latency-class routing** — for interactive and competitive workloads, placement optimizes round-trip time and jitter, not only throughput (see Critical Latency Future Use Cases).

Datacenters still win on absolute throughput and on jobs that need dense interconnect. Little miners still win on aggregate geographic spread, elastic overflow, censorship resistance, edge proximity for millisecond budgets, and a protected slice of emissions. Both are features.

## Workload Wedge Research Assumption

Full planetary orchestration for all workload classes is not day-one feasible. We assume a **phased wedge** that grows from measurable batch work into latency-critical interactive systems:

1. **Phase A:** containerized batch AI inference and fine-tuning jobs with deterministic or challengeable outputs.
2. **Phase B:** offline rendering / frame farms (embarrassingly parallel).
3. **Phase C:** broader simulation and spatial pipelines as coordination latency budgets improve.
4. **Phase D:** **critical-latency interactive fabric** — competitive P2P gaming, live service authority, cloud game streaming assists, and real-time spatial sessions where milliseconds decide winners.

Verification difficulty increases from A→D; token reward weights for \(F_{i,t}\) and latency reputation increase for harder-to-check and tighter-SLA classes.

## Critical Latency Future Use Cases

Batch supercomputing is necessary—but not sufficient—for the network GRID is building. A large class of future demand is **highly critical**: if the fabric is late, the product fails, not degrades politely. Competitive play is the clearest public example. In ranked multiplayer, a few milliseconds of extra lag are not a UX inconvenience; they are a competitive injury. The same physics applies to live co-op worlds, cloud-assisted rendering, and spatial sessions where bodies and cameras move in real time.

### Why milliseconds change the topology

For batch AI training, a GPU in another hemisphere is often fine. For competitive P2P gaming, the right node is frequently **the good GPU near the players**, not the biggest cluster far away. That inverts the usual cloud hierarchy:

| Workload class | Dominant metric | Preferential supply |
| --- | --- | --- |
| Batch train / offline render | Throughput, cost | Dense datacenter `L` + rack `M` |
| Interactive inference | Latency + cost | Regional mix of `M`/`L` |
| **Competitive P2P / esports-grade** | **RTT, jitter, packet path** | **Edge `S`/`M` near players; relays with proven latency SLAs** |
| Live spatial / AR multi-user | Latency + fidelity | Geo-local node meshes |
| Cloud game assist (encode/upscale) | Frame deadline (ms) | Low-jitter edge GPUs |

Little miners are not charity cases in this future—they are **latency infrastructure**. A bedroom rig on the right metropolitan fiber path can beat a distant H100 farm for a 64-player ranked match’s simulation or anti-cheat sidecar. Datacenters still matter for heavy world simulation, matchmaking backends, and VOD-scale rendering; the winning architecture is **hybrid placement** under one token economy.

### Competitive and interactive use cases (assumed roadmap)

1. **Competitive P2P gaming (ms-critical)**  
   Authority shards, physics ticks, hit registration assist, and trusted relay nodes placed to minimize RTT between matched players. Buyers purchase **latency-bounded capacity**, not generic FLOPs. Nodes that miss tick deadlines lose fidelity score \(F_{i,t}\) and latency reputation—even if average throughput looks fine.

2. **Esports and ranked integrity**  
   Real-time anomaly detection, replay hash verification, and anti-cheat feature extraction co-located with match traffic so competitive integrity does not add a cross-continent round trip.

3. **Cloud and hybrid game streaming**  
   Frame upscaling, AV1/HEVC encode, and last-mile GPU assist with hard frame budgets (e.g. 16.7 ms class pathways). Missed deadlines are failed work, not partial credit.

4. **Persistent multiplayer / live service worlds**  
   Shard simulation for dense zones; spin edge capacity when a city-scale event spikes concurrent players.

5. **Spatial computing and shared AR/VR**  
   Pose fusion, occlusion, and environment reconstruction near users so multi-user sessions stay locked in time.

6. **Interactive creation**  
   Live collaborative DCC (digital content creation), real-time ray assist, and stage visualization for virtual production—where directors notice lag as broken creative flow.

7. **Safety-adjacent edge inference** (longer horizon)  
   Local model execution for robotics demos, industrial AR, and emergency simulation sandboxes where WAN failover is too slow for the control loop.

### Latency-aware economics (assumptions)

Extend buyer quotes with a service class \(\ell \in \{\text{batch}, \text{interactive}, \text{critical}\}\). For critical jobs, the coordination layer solves placement under a latency budget \(L_{\max}\) (milliseconds) and a jitter bound \(J_{\max}\):

$$
\text{feasible}(i, k) \iff \widehat{\text{RTT}}_{i,k} \le L_{\max} \;\wedge\; \widehat{J}_{i,k} \le J_{\max} \;\wedge\; \text{capable}(i, k)
$$
{#eq-latency-feasibility title="Critical Job Feasibility Predicate"}

<variables for="eq-latency-feasibility">
  <variable symbol="$\widehat{\text{RTT}}_{i,k}$">estimated round-trip time between node $i$ and session locus $k$</variable>
  <variable symbol="$\widehat{J}_{i,k}$">estimated latency jitter for the same path</variable>
  <variable symbol="$L_{\max}$">maximum acceptable RTT for the job service class</variable>
  <variable symbol="$J_{\max}$">maximum acceptable jitter</variable>
  <variable symbol="$\text{capable}(i, k)$">hardware and sandbox capability match for the workload</variable>
</variables>

**Premium pricing assumption:** critical-latency jobs pay a multiplier \(\beta > 1\) on \(\pi_t\) (assumed \(\beta \in [1.5, 3.0]\) by region scarcity). That premium flows through escrow to the edge nodes that can actually hit the budget—often little miners and regional racks—creating a durable economic reason for geographic diversity beyond the inclusion pool alone.

**Reputation assumption:** nodes publish (and are challenged on) latency histograms. Chronic SLA misses slash latency reputation used in critical matching, separate from batch throughput reputation. A machine can be excellent for overnight renders and ineligible for ranked match ticks.

```arcmark
nodes:
  players: "Players / Interactive Clients"
  edge: "Edge Little Miners (ms path)"
  regional: "Regional Racks"
  dense: "Datacenter Dense GPUs"
  match: "Latency-Aware Scheduler"
  settle: "Escrow + Premium GRID Pay"
links:
  players -> match
  match -> edge
  match -> regional
  match -> dense
  edge -> settle
  regional -> settle
  dense -> settle
```
{#diag-latency-fabric title="Critical-Latency Hybrid Placement"}

### Design thesis

The future GRID is a **planetary computer that can also be a planetary edge**. Competitive P2P gaming is the emotional and technical stress test: if the network can keep fair, fast play when milliseconds matter, it can host the broader class of live digital worlds. Token demand then attaches not only to “how many GPUs” but to **where trustworthy GPUs sit on the map**—and that is how little miners stay indispensable after datacenters join.

## Competitive Landscape Positioning

| Network class | Typical reward basis | GRID differentiation (claim of design) |
| --- | --- | --- |
| PoW chains | Hash puzzles | Useful work + application layer |
| Pure GPU rental markets | USD invoices | Native token metering + global node autonomy |
| Render / DePIN peers | Mixed | PoR scoring, reputation, escrow burn sink, explicit fiat exit path |

GRID does not claim uniqueness of “decentralized GPUs”; it claims a coherent **fabric + PoR + token + marketplace** stack oriented to planetary orchestration.

## Growth Flywheel

```arcmark
nodes:
  supply: "More verified nodes"
  capacity: "Higher Q capacity"
  buyers: "More app demand"
  spend: "More GRID locked in escrow"
  income: "Higher node USD income"
  liquid: "Deeper token markets"
links:
  supply -> capacity
  capacity -> buyers
  buyers -> spend
  spend -> income
  income -> supply
  spend -> liquid
  liquid -> income
```
{#diag-flywheel title="Supply–Demand–Liquidity Flywheel"}

Bootstrap capital is spent on **software, audits, liquidity, and temporary demand subsidies**, not on owning all GPUs. That is how the network answers the founder problem of needing both capital and compute: **the market supplies compute; the token coordinates payment; the marketplace converts rewards into money.**

## Operator Unit Economics (Illustrative Assumptions)

**Little miner (class `S`, 1× consumer GPU):**

- Verified utilization: 40% of hours in epoch window during bootstrap
- Income stack: proportional emission + **inclusion pool** + job shards
- Emission + job share: **target** \$50–\$150/month USD-equivalent in year 0 after costs, region-dependent
- Bond: low (assumed 250 GRID) so ex-ETH home miners can onboard without warehouse capital

**Former mining rack (class `M`):** higher absolute GRID from proportional rewards; same marketplace exit; ideal bridge population for bootstrap.

**Datacenter (class `L`):** largest proportional rewards and contiguous job fills; no claim on the inclusion pool; still subject to anti-monopoly caps when the network has idle small capacity.

Power cost is paid by every operator in fiat; efficiency score \(E_{i,t}\) rewards thrifty hardware—another natural fit for miners who already optimize joules.

These figures are **planning targets**, not guarantees. Real yields depend on hardware, electricity, utilization, token price, and demand. The emission controller and subsidy budget are the dials used to approach targets during bootstrap without promising fixed USD returns (which would break protocol neutrality).

## Compliance and Cash-Out Path

“Crypto” is the correct informal term for digital assets such as GRID, BTC, ETH, and USDC. Operationally:

1. Operator earns **GRID** (cryptocurrency / utility token).
2. Operator sells GRID on the **token marketplace** for USDC or ETH.
3. Operator optionally converts USDC to **fiat** via regulated exchanges or payment partners.

The protocol provides open settlement and optional routing UI; it does not need to be a bank. Jurisdiction-specific KYC applies at fiat edges, not at pure peer compute contribution.

# Results

This section states design outcomes as **claims** of the specification (to be validated empirically on mainnet), not historical measurements.

<claim id="claim-utility-01" type="theoretical" confidence="high">
Binding network capacity purchases to GRID escrow creates continuous transactional demand for the token proportional to real compute usage.
</claim>

<claim id="claim-por-01" type="theoretical" confidence="high">
Allocating emissions proportional to reputation-weighted Proof-of-Resource scores \(S_{i,t}\) directs rewards toward useful, reliable hardware rather than pure capital lockup or hash puzzles.
</claim>

<claim id="claim-sink-01" type="theoretical" confidence="medium">
A 10% burn on successful job settlement introduces a structural supply sink that scales with network usage, partially offsetting emission during growth.
</claim>

<claim id="claim-liquidity-01" type="theoretical" confidence="medium">
Seeding GRID/USDC liquidity and exposing a simple marketplace exit path is necessary for sustained node participation, because operators ultimately pay electricity and hardware costs in fiat.
</claim>

<claim id="claim-bootstrap-01" type="theoretical" confidence="medium">
Front-loading emissions in years 0–2, combined with demand subsidies, can cold-start supply before organic buyer volume dominates operator income.
</claim>

<claim id="claim-miner-continuity-01" type="theoretical" confidence="high">
Proof-of-Resource lets operators with working GPUs—especially former Ethereum GPU miners and Bitcoin-era operators who retain GPU fleets—continue mining for network rewards by producing verified useful compute instead of non-productive puzzles.
</claim>

<claim id="claim-inclusion-01" type="theoretical" confidence="medium">
Reserving a dedicated inclusion emission pool for active small-class nodes, combined with job sharding and per-operator assignment caps, allows datacenters to join at scale without extinguishing little-miner participation.
</claim>

<claim id="claim-latency-01" type="theoretical" confidence="medium">
Latency-bounded placement with premium pricing for critical service classes makes geographically distributed little miners economically essential for competitive P2P gaming and other millisecond-sensitive workloads, rather than optional overflow for batch jobs.
</claim>

<claim id="claim-hybrid-01" type="theoretical" confidence="high">
A hybrid fabric that routes batch work to dense capacity and interactive critical work to feasible edge nodes under RTT and jitter constraints better matches real digital-world demand than throughput-only scheduling.
</claim>

<claim id="claim-emission-cap-01" type="theoretical" confidence="high">
Fixed epoch emission budgets, a hard max supply, and a per-operator-cluster proportional mint ceiling prevent unbounded token disbursement even if a hyperscale operator joins with vast hardware.
</claim>

<claim id="claim-genesis-earn-01" type="theoretical" confidence="medium">
A one-year Genesis Earn phase—miners earn under vesting locks while open token-for-capacity spend stays closed—bootstraps measured supply without a separate testnet economy and without free unlimited mint-and-dump dynamics.
</claim>

## Parameter Summary (v1 Assumptions)

| Knob | v1 value |
| --- | --- |
| Max supply | 10B GRID |
| Emission bucket | 45% over 10 years |
| Epoch | 1 hour |
| Score weights \((w_c,w_u,w_e,w_f)\) | (0.55, 0.15, 0.10, 0.20) |
| Emission split (proportional / inclusion) | 90% / 10% |
| Settlement split (nodes / burn / treasury) | 85% / 10% / 5% |
| Anti-monopoly assignment cap \(\kappa\) | 15% per operator cluster |
| Little-miner bond (class `S`) | 250 GRID |
| Price elasticity \(\alpha\) | 0.7 |
| Contributor vest | 1y cliff / 4y linear |
| Transact Security Layer | **Bitcoin** |
| Utility rail | Fast L2 / app-chain (impl. choice) |
| Primary exit pair | **GRID/BTC** (USDC for fiat legs) |
| Critical latency premium \(\beta\) | 1.5×–3.0× base \(\pi_t\) |
| Workload phases | A batch AI → B render → C sim/spatial → D critical interactive |
| Per-cluster emission ceiling \(\gamma\) | 5% of proportional pool per epoch |
| Genesis Earn Year | Months 0–12 earn-under-lock; open spend after |

# Limitations

1. **Parameter uncertainty** — weights, splits, and emission decay are assumptions; adversarial behavior and real workload mix may force retuning.
2. **Verification hardness** — not all compute is cheaply verifiable; some jobs require redundancy or optimistic challenges, increasing cost and latency.
3. **Token price volatility** — operator fiat income fluctuates if rewards are paid in GRID; optional stablecoin billing with protocol auto-buy of GRID is a future mitigation.
4. **Regulatory surface** — token distribution, airdrops, and fiat ramps require legal review per jurisdiction; this paper is technical, not a securities opinion.
5. **Marketplace dependency** — without external liquidity and off-ramps, the economic loop fails regardless of elegant on-chain design.
6. **Cross-chain complexity** — multi-chain nodes and bridged GRID introduce bridge risk; v1 assumes a single primary L2.
7. **No guaranteed yield** — emissions and prices are market outcomes; the protocol cannot promise fixed USD returns.
8. **Governance lag** — updating \(w_\ast\), burn rate, or emission curves requires careful process to avoid capture or instability.
9. **Non-shardable jobs** — some datacenter-only workloads (tight multi-GPU interconnect) cannot be split to little miners; inclusion then relies more on emission pool and other job classes.
10. **Class gaming** — operators may try to split a datacenter into many fake “small” identities; identity clustering, stake correlation, and collocation heuristics must defend the inclusion pool.
11. **Hard real-time guarantees** — home networks vary; competitive gaming SLAs require measurement, ejection of chronic under-performers, and honest admission that not every little miner qualifies for every ranked path.
12. **Last-mile variance** — ISP routing and Wi-Fi noise can dominate GPU speed for ms-critical sessions; latency reputation must track path quality, not only device benchmarks.

# Conclusion

GRID’s token layer turns a distributed computer into a market. Proof-of-Resource measures real contribution; escrow converts demand into node income; burns and treasury fees align long-term supply with usage; and a token marketplace lets participants sell GRID for fiat or other cryptocurrencies—the practical answer to “can I get paid for my compute?”

The outstanding social and technical result is **miner continuity with useful work**: Ethereum GPU miners and Bitcoin-era operators with working GPUs can keep mining—same discipline, new product—while datacenters join to expand capacity. Inclusive scheduling, progressive bonds, anti-monopoly assignment caps, and a dedicated little-miner emission pool exist so the home rig is not erased when the warehouse comes online.

The outstanding product result is a path from batch capacity to **critical-latency digital life**: competitive P2P gaming, live worlds, and spatial sessions where milliseconds are the market. In that future, edge miners are not nostalgia—they are how the fabric stays fast enough to feel fair.

By assuming a hard-capped ERC-20 on a low-fee L2, a 45% ten-year emission budget for node operators, reputation-weighted rewards, a 90/10 proportional–inclusion emission split, latency-bounded placement with premium critical pricing, and an 85/10/5 settlement split, this specification provides a complete, implementable path from white-paper vision to economic mainnet. The protocol does not need to own planetary hardware; it needs to **measure work, settle value, protect open participation, honor the miner, and keep exit liquidity honest**.

Future work includes mainnet calibration of \(R_{i,t}\), formal verification of escrow contracts, stablecoin-quoted jobs with automatic GRID conversion, measurement of little-miner share under datacenter load, latency SLA experiments for competitive session placement, and governance procedures for parameter changes after empirical measurement.

# References

1. Caraveo, J. *GRID: World's First Planetary Supercomputer* (GRID White Paper), 2026. Especially Chapter 8: GRID Resource Economy & Marketplace.
2. Nakamoto, S. *Bitcoin: A Peer-to-Peer Electronic Cash System*, 2008.
3. Buterin, V. *Ethereum: A Next-Generation Smart Contract and Decentralized Application Platform*, 2014.
4. OpenResearch Initiative. *Universal Research Paper (URP) Specification*, v0.1, 2026.
5. Related DePIN / distributed compute systems (contextual landscape): Render Network docs; Akash Network docs; Golem Network docs; io.net docs — used as comparative market context, not as normative design sources.
