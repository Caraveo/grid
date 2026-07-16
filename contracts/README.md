# GRID Contracts (P3 stubs)

**Do not deploy to mainnet without audit.**

Planned modules (see `technical.md` + token spec):

| Contract | Role |
| --- | --- |
| `GRIDToken.sol` | Capped ERC-20 |
| `EmissionController.sol` | Epoch mint, γ whale cap, inclusion pool |
| `GenesisLock.sol` | Year-1 earn vesting |
| `NodeRegistry.sol` | Bonds + class |
| `JobEscrow.sol` | Post–Genesis capacity payments |

Stubs land in a follow-up when P1 demo is stable. Prefer OpenZeppelin + Foundry.
