# Phase 0 — Genesis Peer Authority

**Your first task as genesis node: Tracking Peers + Banning Peers only.**

You are the **source of truth** for who is tracked and who is banned. Nothing else (jobs, tokens, money) is under this authority in Phase 0.

Bitcoin remains the **Transact Security Layer** for value. Genesis is **peer-policy** only.

---

## Threat model (what we defend)

| Attack | Mitigation |
|--------|------------|
| Peer forges a ban list | Ed25519 signature over canonical snapshot; peers verify against **your** pubkey |
| Replay old “empty ban list” | Monotonic **epoch**; peers reject lower epochs |
| Remote attacker bans someone | **No remote ban API** — ban only via local CLI with `secret.key` on your machine |
| Announce spam becomes “truth” | `POST /v1/announce` is log-only; you must `grid genesis track` to promote |
| Secret key theft | File mode `0600`; never leave genesis host; never commit secret |
| Tamper mid-wire | Signature fails verification |

---

## What only genesis can do

1. **Track** a peer (id, name, listen, class)  
2. **Ban** a peer (id + reason) — removes from tracked  
3. **Unban / untrack**  
4. **Serve** signed truth (`GET /v1/truth`)

Peers can:

- Fetch and **verify** truth  
- **Enforce** bans on P2P hello  
- Announce themselves (request visibility) — not authority  

---

## Operator runbook (you)

```bash
# 1) Create keys once (on your secure machine)
grid genesis init
# → ~/.grid/genesis/secret.key  (NEVER share)
# → ~/.grid/genesis/public.key  (distribute this)

# 2) Publish truth
grid genesis serve --bind 0.0.0.0:9100

# 3) Track peers you accept
grid genesis track --id bob-1 --name bob --listen 1.2.3.4:9900 --class S

# 4) Ban bad actors
grid genesis ban --id evil-9 --reason "spam / abuse"

grid genesis list
grid genesis pubkey
```

Distribute to the network:

```bash
export GRID_GENESIS=http://YOUR_IP:9100
export GRID_GENESIS_PUBKEY=$(grid genesis pubkey)
```

---

## Peer runbook

```bash
# Trust only this pubkey (pin it)
export GRID_GENESIS=http://genesis.example:9100
export GRID_GENESIS_PUBKEY=...hex...

grid peer --listen 0.0.0.0:9900 \
  --genesis "$GRID_GENESIS" \
  --genesis-pubkey "$GRID_GENESIS_PUBKEY"

# Verify truth yourself
grid genesis truth --url "$GRID_GENESIS" --pubkey "$GRID_GENESIS_PUBKEY"
```

If a peer is banned, hello is **REJECT**’d and connections dropped on truth refresh.

---

## Snapshot shape (signed)

```json
{
  "epoch": 7,
  "issued_at": "...",
  "genesis_pubkey": "...",
  "tracked": [ { "peer_id", "name", "listen", "class", "tracked_at" } ],
  "banned": [ { "peer_id", "reason", "banned_at", "ban_id" } ],
  "signature": "ed25519 hex over blake3(canonical body JSON)"
}
```

---

## Phase 0 boundaries

**In scope:** track + ban + signed distribution + P2P enforce  
**Out of scope:** job routing authority, token mint, automated slashing, multi-sig genesis  

Later phases can multi-sig or commit ban-root hashes to **Bitcoin TSL** for public audit — not required for Phase 0 MVP.
