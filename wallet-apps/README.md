# Phoenix — GRID Wallet applications

Phoenix, the official GRID Wallet suite, uses one Rust wallet contract:

- `macos/` — native SwiftUI application with system/light/dark themes.
- `desktop/` — Tauri application for Windows and Linux.
- `grid gui snapshot|action` — local JSON bridge used by SwiftUI. Secret-bearing
  actions are accepted over stdin and are never placed in process arguments.

All applications operate on the existing `~/.grid` vault, `wallet.json`, and
`chain.json`. They do not create a competing browser wallet.

## Custody

- GRID keys remain protected by the existing passkey, password, 24-word
  keyphrase, or combo vault.
- The 24-word phrase is returned once during setup and is not persisted.
- Imported Solana wallets are watch-only. Locally created Solana reward keys
  remain under `~/.grid/keys/solana-reward.json`.
- Bitcoin consolidation is labelled roadmap until audited execution and
  liquidity are live.
