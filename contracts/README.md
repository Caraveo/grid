# GRID settlement contracts

**Do not deploy to mainnet without audit.**

GRID's first utility rail is Solana. The devnet implementation now lives in the
standalone workspace at `../../grid-solana/` and creates a standard SPL token
while preserving the existing GRID issuance rules:

- 10 billion GRID maximum circulating supply
- 10,000 GRID maximum issuance per one-hour epoch
- one reward per verified job ID
- no freeze authority
- devnet-only deployment guard

The current issuer is deliberately an operator-controlled **prototype relayer**,
not a trustless emission controller. It must not be used for a public sale or
mainnet launch. An audited Solana program and multisig mint authority are required
before mainnet.
