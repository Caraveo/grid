#!/usr/bin/env bash
# GRID installer — useful mining CLI
# Usage:
#   curl -fsSL https://raw.githubusercontent.com/Caraveo/grid/master/scripts/install.sh | bash
#   curl -fsSL https://raw.githubusercontent.com/Caraveo/grid/master/scripts/install.sh | bash -s -- --from-source
#
# No separate public testnet economy. Mainnet path: software pilot → Genesis Earn → open utility.
set -euo pipefail

REPO="${GRID_REPO:-https://github.com/Caraveo/grid.git}"
RAW_BASE="${GRID_RAW_BASE:-https://raw.githubusercontent.com/Caraveo/grid/master}"
INSTALL_DIR="${GRID_INSTALL_DIR:-$HOME/.local/bin}"
PREFIX="${GRID_PREFIX:-}"
FROM_SOURCE=0
FORCE=0

for arg in "$@"; do
  case "$arg" in
    --from-source) FROM_SOURCE=1 ;;
    --force) FORCE=1 ;;
    --prefix=*) PREFIX="${arg#*=}" ;;
    -h|--help)
      cat <<'EOF'
GRID install

  curl -fsSL https://raw.githubusercontent.com/Caraveo/grid/master/scripts/install.sh | bash

Options (pass after bash -s --):
  --from-source   Always build with cargo from git
  --force         Reinstall even if grid exists
  --prefix=DIR    Install binary to DIR (default: ~/.local/bin)

Env:
  GRID_REPO            git URL (default: https://github.com/Caraveo/grid.git)
  GRID_INSTALL_DIR     install directory (default: ~/.local/bin)
  GRID_VERSION         cargo/git tag or branch (default: master)
EOF
      exit 0
      ;;
  esac
done

VERSION="${GRID_VERSION:-master}"
bold() { printf '\033[1m%s\033[0m\n' "$*"; }
info() { printf '→ %s\n' "$*"; }
die()  { printf 'error: %s\n' "$*" >&2; exit 1; }

need_cmd() {
  command -v "$1" >/dev/null 2>&1 || die "missing required command: $1"
}

os_arch() {
  local os arch
  os="$(uname -s | tr '[:upper:]' '[:lower:]')"
  arch="$(uname -m)"
  case "$arch" in
    x86_64|amd64) arch="x86_64" ;;
    aarch64|arm64) arch="aarch64" ;;
    *) die "unsupported arch: $arch" ;;
  esac
  case "$os" in
    linux|darwin) ;;
    *) die "unsupported OS: $os (use --from-source on supported hosts)" ;;
  esac
  echo "${os}-${arch}"
}

resolve_bin_dir() {
  if [[ -n "$PREFIX" ]]; then
    echo "$PREFIX"
  else
    echo "$INSTALL_DIR"
  fi
}

install_binary() {
  local src="$1"
  local dest_dir
  dest_dir="$(resolve_bin_dir)"
  mkdir -p "$dest_dir"
  install -m 755 "$src" "$dest_dir/grid"
  bold "Installed: $dest_dir/grid"
  if ! echo ":$PATH:" | grep -q ":$dest_dir:"; then
    info "Add to PATH:"
    echo "  export PATH=\"$dest_dir:\$PATH\""
    echo "  # e.g. add that line to ~/.zshrc or ~/.bashrc"
  fi
}

try_prebuilt() {
  # When GitHub Releases publish assets named grid-<os>-<arch>, this path works.
  local platform asset url tmp
  platform="$(os_arch)"
  asset="grid-${platform}"
  url="https://github.com/Caraveo/grid/releases/latest/download/${asset}"
  tmp="$(mktemp -d)"
  info "Trying prebuilt: $url"
  if curl -fsSL "$url" -o "$tmp/grid" 2>/dev/null; then
    chmod +x "$tmp/grid"
    if "$tmp/grid" --version >/dev/null 2>&1; then
      install_binary "$tmp/grid"
      rm -rf "$tmp"
      return 0
    fi
  fi
  rm -rf "$tmp"
  return 1
}

build_from_source() {
  need_cmd git
  need_cmd cargo
  local work
  work="$(mktemp -d)"
  info "Cloning $REPO ($VERSION)…"
  git clone --depth 1 --branch "$VERSION" "$REPO" "$work/grid" 2>/dev/null \
    || git clone --depth 1 "$REPO" "$work/grid"
  info "Building release (this may take a few minutes)…"
  (cd "$work/grid" && cargo build --release --locked 2>/dev/null || cargo build --release)
  install_binary "$work/grid/target/release/grid"
  rm -rf "$work"
}

main() {
  bold "GRID installer"
  info "Useful mining · Bitcoin Transact Security Layer · no public testnet economy"

  if command -v grid >/dev/null 2>&1 && [[ "$FORCE" -eq 0 ]]; then
    info "grid already on PATH: $(command -v grid) ($(grid --version 2>/dev/null || echo present))"
    info "Re-run with --force to reinstall:  bash -s -- --force"
    exit 0
  fi

  if [[ "$FROM_SOURCE" -eq 1 ]]; then
    build_from_source
  else
    if ! try_prebuilt; then
      info "No prebuilt binary for this platform (or release not published yet)."
      info "Building from source…"
      build_from_source
    fi
  fi

  echo
  bold "Next steps"
  cat <<'EOF'
  grid --help
  grid init --name my-node --class S
  grid coord          # terminal 1 — coordinator
  grid node           # terminal 2 — mine useful work
  grid submit --wait  # terminal 3 — submit a job

Network economy: mainnet path only (Genesis Earn → open utility).
No separate public testnet token economy.
EOF
}

main
