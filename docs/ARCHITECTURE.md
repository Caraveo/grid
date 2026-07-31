# GRID architecture

```mermaid
flowchart TB
    Operators["Node operators<br/>GRID CLI"] --> Gate["Signed minimum-version gate"]
    Wallets["ARK native wallets<br/>Local keys + local signing"] --> Edge["ark.grid-compute.com<br/>Signed envelopes only"]
    Edge --> Verify["Signature, chain, nonce,<br/>and replay verification"]
    Gate --> Core["GRID node<br/>host + mine + P2P"]
    Core --> Coordinator["Useful-compute coordinator"]
    Coordinator --> Receipts["Verified settlement receipts"]
    Verify --> Genesis
    Receipts --> Genesis["Genesis producer<br/>Canonical Phase 1 chain"]
    Genesis --> Peers["Independent P2P replicas"]
    Peers -->|"verify signatures, links,<br/>state roots + allocation"| Genesis
```

## Trust boundaries

- Wallet secrets remain on the device. Network services receive signed
  transaction envelopes, never private keys.
- Genesis signs the current minimum supported CLI version. `grid host`,
  `grid mine`, and `grid node` verify that signed policy before starting.
- The coordinator independently rejects claims from missing or outdated CLI
  versions, so older clients cannot bypass the local startup check.
- Genesis currently produces canonical Phase 1 blocks. Peers replicate and
  verify them; permissionless multi-validator finality remains a mainnet gate.

The live, visual contributor overview is maintained at
<https://docs.grid-compute.com/network#overview>.
