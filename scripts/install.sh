#!/usr/bin/env bash
# GRID installer — Phase 1 useful mining CLI
#
# One-liner:
#   curl -fsSL https://raw.githubusercontent.com/Caraveo/grid/master/scripts/install.sh | bash
#
# Local (from a clone / this repo):
#   ./scripts/install.sh --local
#   make install
#
# Options (after bash -s -- …):
#   --force         Reinstall even if Phase-1 grid is already present
#   --from-source   Always cargo-build from git (skip prebuilt)
#   --local         Build from the repo that contains this script
#   --system        Prefer /usr/local/bin (uses sudo if needed)
#   --prefix=DIR    Install directory (default: ~/.local/bin)
#   --uninstall     Remove Phase-1 grid binaries we manage
#   -h | --help
#
# Env:
#   GRID_REPO          git URL (default: https://github.com/Caraveo/grid.git)
#   GRID_VERSION       git branch/tag (default: master)
#   GRID_INSTALL_DIR   install directory (default: ~/.local/bin)
#   GRID_PREFIX        same as --prefix
set -euo pipefail

REPO="${GRID_REPO:-https://github.com/Caraveo/grid.git}"
RAW_BASE="${GRID_RAW_BASE:-https://raw.githubusercontent.com/Caraveo/grid/master}"
INSTALL_DIR="${GRID_INSTALL_DIR:-$HOME/.local/bin}"
PREFIX="${GRID_PREFIX:-}"
FROM_SOURCE=0
FORCE=0
LOCAL=0
SYSTEM=0
UNINSTALL=0

# Resolve directory of this script when run from a clone (not curl|bash).
SCRIPT_DIR=""
if [[ -n "${BASH_SOURCE[0]:-}" && -f "${BASH_SOURCE[0]}" ]]; then
  SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
fi
REPO_ROOT=""
if [[ -n "$SCRIPT_DIR" && -f "$SCRIPT_DIR/../Cargo.toml" ]]; then
  REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
fi

for arg in "$@"; do
  case "$arg" in
    --from-source) FROM_SOURCE=1 ;;
    --force) FORCE=1 ;;
    --local) LOCAL=1 ;;
    --system) SYSTEM=1 ;;
    --uninstall) UNINSTALL=1 ;;
    --prefix=*) PREFIX="${arg#*=}" ;;
    -h|--help)
      cat <<'EOF'
GRID install — Phase 1 useful mining CLI

  curl -fsSL https://raw.githubusercontent.com/Caraveo/grid/master/scripts/install.sh | bash

From a clone:
  ./scripts/install.sh --local
  make install

Options (pass after bash -s --):
  --force         Reinstall even if Phase-1 grid already works
  --from-source   Always build with cargo from git
  --local         Build from the repository containing this script
  --system        Prefer /usr/local/bin (may use sudo)
  --prefix=DIR    Install binary into DIR (default: ~/.local/bin)
  --uninstall     Remove managed grid binaries
  -h, --help      This help

Env:
  GRID_REPO            git URL
  GRID_INSTALL_DIR     install directory (default: ~/.local/bin)
  GRID_VERSION         cargo/git tag or branch (default: master)
  GRID_PREFIX          same as --prefix

After install, verify:
  hash -r && which grid && grid -V && grid auth --help
EOF
      exit 0
      ;;
    *)
      printf 'error: unknown option: %s\n' "$arg" >&2
      exit 1
      ;;
  esac
done

VERSION="${GRID_VERSION:-master}"

# ─── pretty ──────────────────────────────────────────────────────────────────
if [[ -t 1 ]]; then
  BOLD=$'\033[1m'; DIM=$'\033[2m'; GREEN=$'\033[32m'; YELLOW=$'\033[33m'
  RED=$'\033[31m'; RESET=$'\033[0m'
else
  BOLD=""; DIM=""; GREEN=""; YELLOW=""; RED=""; RESET=""
fi
# Progress → stderr so $(build_*) only captures the binary path on stdout.
bold()  { printf '%s%s%s\n' "$BOLD" "$*" "$RESET" >&2; }
info()  { printf '%s→%s %s\n' "$DIM" "$RESET" "$*" >&2; }
ok()    { printf '%s✓%s %s\n' "$GREEN" "$RESET" "$*" >&2; }
warn()  { printf '%s!%s %s\n' "$YELLOW" "$RESET" "$*" >&2; }
die()   { printf '%serror:%s %s\n' "$RED" "$RESET" "$*" >&2; exit 1; }

need_cmd() {
  command -v "$1" >/dev/null 2>&1 || die "missing required command: $1"
}

# ─── Phase-1 detection ───────────────────────────────────────────────────────
# Legacy binaries (old grid-cli) also named `grid` but have no `auth` command.
is_phase1_binary() {
  local bin="$1"
  [[ -x "$bin" ]] || return 1
  # Phase 1 help mentions auth / useful mining / Transact Security
  if "$bin" auth --help >/dev/null 2>&1; then
    return 0
  fi
  return 1
}

first_grid_on_path() {
  command -v grid 2>/dev/null || true
}

list_grid_candidates() {
  # Unique existing paths that might be "grid"
  local -a paths=(
    "$HOME/.local/bin/grid"
    "$HOME/bin/grid"
    "/usr/local/bin/grid"
    "/opt/homebrew/bin/grid"
  )
  local p g
  g="$(first_grid_on_path)"
  [[ -n "$g" ]] && paths+=("$g")
  # type -a style via PATH walk
  local IFS=':'
  for d in $PATH; do
    [[ -n "$d" && -x "$d/grid" ]] && paths+=("$d/grid")
  done
  printf '%s\n' "${paths[@]}" | awk 'NF && !seen[$0]++'
}

# ─── paths ───────────────────────────────────────────────────────────────────
resolve_bin_dir() {
  if [[ -n "$PREFIX" ]]; then
    echo "$PREFIX"
  elif [[ "$SYSTEM" -eq 1 ]]; then
    echo "/usr/local/bin"
  else
    echo "$INSTALL_DIR"
  fi
}

ensure_dir_writable() {
  local dir="$1"
  if [[ -d "$dir" && -w "$dir" ]]; then
    return 0
  fi
  if mkdir -p "$dir" 2>/dev/null && [[ -w "$dir" ]]; then
    return 0
  fi
  return 1
}

# Returns 0 on success, 1 if dest is not writable (caller decides hard-fail vs skip).
install_file() {
  local src="$1"
  local dest="$2"
  local dest_dir
  dest_dir="$(dirname "$dest")"

  if ensure_dir_writable "$dest_dir"; then
    install -m 755 "$src" "$dest"
    return 0
  fi

  # Interactive sudo only (curl|bash / CI must not hang on password)
  if [[ -t 0 ]] && command -v sudo >/dev/null 2>&1; then
    warn "Need elevated rights to write $dest"
    if sudo mkdir -p "$dest_dir" && sudo install -m 755 "$src" "$dest"; then
      return 0
    fi
  fi

  return 1
}

# ─── PATH hygiene ────────────────────────────────────────────────────────────
ensure_path_export() {
  local dest_dir="$1"
  case ":$PATH:" in
    *":$dest_dir:"*) return 0 ;;
  esac

  warn "$dest_dir is not on your PATH"
  info "Add this to your shell config, then open a new terminal:"
  echo
  echo "  export PATH=\"$dest_dir:\$PATH\""
  echo

  local rc=""
  if [[ -n "${ZSH_VERSION:-}" ]] || [[ "${SHELL:-}" == *zsh* ]]; then
    rc="$HOME/.zshrc"
  elif [[ -n "${BASH_VERSION:-}" ]] || [[ "${SHELL:-}" == *bash* ]]; then
    rc="$HOME/.bashrc"
  fi

  # Auto-append once if interactive-friendly and not already present
  if [[ -n "$rc" ]]; then
    local line="export PATH=\"$dest_dir:\$PATH\"  # GRID CLI"
    if [[ -f "$rc" ]] && grep -qF "$dest_dir" "$rc" 2>/dev/null; then
      info "PATH already referenced in $rc"
    else
      if [[ -t 0 || -t 1 ]]; then
        # Non-destructive: only append when install dir is the default user bin
        if [[ "$dest_dir" == "$HOME/.local/bin" || "$dest_dir" == "$HOME/bin" ]]; then
          printf '\n# GRID CLI\n%s\n' "$line" >> "$rc"
          ok "Appended PATH export to $rc"
          export PATH="$dest_dir:$PATH"
        fi
      fi
    fi
  fi
}

# ─── platform / prebuilt ─────────────────────────────────────────────────────
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

try_prebuilt() {
  local platform asset url tmp
  platform="$(os_arch)"
  asset="grid-${platform}"
  url="https://github.com/Caraveo/grid/releases/latest/download/${asset}"
  tmp="$(mktemp "${TMPDIR:-/tmp}/grid-prebuilt.XXXXXX")"
  info "Trying prebuilt: $url"
  if curl -fsSL "$url" -o "$tmp" 2>/dev/null; then
    chmod +x "$tmp"
    if is_phase1_binary "$tmp"; then
      echo "$tmp"
      return 0
    fi
    warn "Downloaded asset is not a Phase-1 grid binary — ignoring"
  fi
  rm -f "$tmp"
  return 1
}

ensure_rust() {
  if command -v cargo >/dev/null 2>&1; then
    return 0
  fi
  warn "cargo not found — installing rustup (stable)"
  need_cmd curl
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --default-toolchain stable
  # shellcheck disable=SC1091
  if [[ -f "$HOME/.cargo/env" ]]; then
    # shellcheck source=/dev/null
    source "$HOME/.cargo/env"
  fi
  command -v cargo >/dev/null 2>&1 || die "cargo still missing after rustup install"
  ok "Rust toolchain ready"
}

build_from_source_git() {
  need_cmd git
  ensure_rust
  local work
  work="$(mktemp -d)"
  info "Cloning $REPO ($VERSION)…"
  if ! git clone --depth 1 --branch "$VERSION" "$REPO" "$work/grid" 2>/dev/null; then
    git clone --depth 1 "$REPO" "$work/grid"
  fi
  info "Building release (this may take a few minutes)…"
  (
    cd "$work/grid"
    if [[ -f Cargo.lock ]]; then
      cargo build --release --locked || cargo build --release
    else
      cargo build --release
    fi
  )
  local bin="$work/grid/target/release/grid"
  [[ -x "$bin" ]] || die "build produced no binary at $bin"
  is_phase1_binary "$bin" || die "built binary failed Phase-1 check (auth subcommand missing)"
  # Keep work dir for copy, then caller cleans — copy out first
  local out
  out="$(mktemp)"
  cp "$bin" "$out"
  chmod 755 "$out"
  rm -rf "$work"
  echo "$out"
}

build_from_local() {
  [[ -n "$REPO_ROOT" ]] || die "--local requires running scripts/install.sh from a git clone"
  ensure_rust
  info "Building from local tree: $REPO_ROOT"
  (
    cd "$REPO_ROOT"
    if [[ -f Cargo.lock ]]; then
      cargo build --release --locked || cargo build --release
    else
      cargo build --release
    fi
  )
  local bin="$REPO_ROOT/target/release/grid"
  [[ -x "$bin" ]] || die "build produced no binary at $bin"
  is_phase1_binary "$bin" || die "local binary failed Phase-1 check"
  echo "$bin"
}

# ─── install / replace ───────────────────────────────────────────────────────
backup_legacy() {
  local path="$1"
  [[ -e "$path" ]] || return 0
  if is_phase1_binary "$path"; then
    return 0
  fi
  local bak="${path}.legacy.bak"
  info "Backing up non-Phase-1 grid: $path → $bak"
  if [[ -w "$(dirname "$path")" ]]; then
    cp "$path" "$bak" 2>/dev/null || true
  elif command -v sudo >/dev/null 2>&1; then
    sudo cp "$path" "$bak" 2>/dev/null || true
  fi
}

replace_conflicting_grids() {
  local new_bin="$1"
  local dest_dir="$2"
  local primary="$dest_dir/grid"

  # Always install primary target (hard fail)
  backup_legacy "$primary"
  if ! install_file "$new_bin" "$primary"; then
    die "cannot write $primary (try --prefix=\"\$HOME/.local/bin\" or --system with sudo)"
  fi
  ok "Installed $primary"

  # Also replace other PATH copies that would shadow us (legacy gotchas)
  local candidate first
  while IFS= read -r candidate; do
    [[ -n "$candidate" ]] || continue
    [[ "$candidate" == "$primary" ]] && continue
    case "$candidate" in
      /usr/local/bin/grid|/opt/homebrew/bin/grid|"$HOME/bin/grid"|"$HOME/.local/bin/grid")
        if ! is_phase1_binary "$candidate"; then
          backup_legacy "$candidate"
          info "Replacing legacy binary at $candidate"
          if ! install_file "$new_bin" "$candidate"; then
            warn "could not replace $candidate (sudo/permissions) — leave it or: sudo rm $candidate"
          fi
        elif [[ "$FORCE" -eq 1 ]]; then
          info "Updating Phase-1 binary at $candidate"
          install_file "$new_bin" "$candidate" || true
        fi
        ;;
    esac
  done < <(list_grid_candidates)

  # If `which grid` still points at legacy elsewhere, warn loudly
  hash -r 2>/dev/null || true
  first="$(first_grid_on_path)"
  if [[ -n "$first" ]] && ! is_phase1_binary "$first"; then
    warn "PATH still resolves to a non-Phase-1 binary: $first"
    warn "Fix: export PATH=\"$dest_dir:\$PATH\"  or  sudo rm \"$first\""
    warn "Then: hash -r && which grid && grid auth --help"
  fi
}

verify_install() {
  hash -r 2>/dev/null || true
  local g
  g="$(first_grid_on_path)"
  if [[ -z "$g" ]]; then
    die "grid not found on PATH after install — open a new shell or export PATH"
  fi
  if ! is_phase1_binary "$g"; then
    die "grid on PATH ($g) is not Phase-1 (missing 'auth'). Run: hash -r && which -a grid"
  fi
  ok "Verified: $g ($("$g" -V 2>/dev/null || echo phase-1))"
  info "Commands: grid auth · grid node · grid bench · grid genesis · grid peer"
  # Show auth is real
  if "$g" auth --help >/dev/null 2>&1; then
    ok "grid auth is available"
  fi
}

do_uninstall() {
  bold "GRID uninstall"
  local removed=0
  local p
  for p in \
    "$HOME/.local/bin/grid" \
    "$HOME/bin/grid" \
    "/usr/local/bin/grid" \
    "/opt/homebrew/bin/grid"
  do
    if [[ -e "$p" ]] && is_phase1_binary "$p"; then
      info "Removing $p"
      if [[ -w "$(dirname "$p")" ]]; then
        rm -f "$p"
      else
        sudo rm -f "$p"
      fi
      removed=$((removed + 1))
    fi
  done
  if [[ "$removed" -eq 0 ]]; then
    warn "No Phase-1 grid binaries found in standard locations"
  else
    ok "Removed $removed binary(ies). Config in ~/.grid was left intact."
  fi
}

print_next() {
  echo
  bold "Next steps"
  cat <<'EOF'
  hash -r
  which grid && grid -V
  grid auth --help

  # Protect keys (pick one)
  grid auth                 # default = passkey
  grid auth master          # password + passkey + 24 words + master key

  # Mine
  grid init --name my-node --class S
  grid coord                # terminal 1
  grid node                 # terminal 2
  grid submit --wait        # terminal 3

Public mesh registry: https://grid-compute.com
  grid registry                 # list peers
  # join globe (location only — never IPs):
  # write ~/.grid/env (mode 600) with GRID_WEBHOOK_SECRET + GRID_GLOBE_LAT/LNG

Docs: https://github.com/Caraveo/grid
Site / registry: https://grid-compute.com
EOF
}

# ─── main ────────────────────────────────────────────────────────────────────
main() {
  bold "GRID installer"
  info "Phase 1 · useful mining · Bitcoin Transact Security Layer"
  info "No separate public testnet economy"

  if [[ "$UNINSTALL" -eq 1 ]]; then
    do_uninstall
    exit 0
  fi

  local existing
  existing="$(first_grid_on_path)"
  if [[ -n "$existing" ]] && is_phase1_binary "$existing" && [[ "$FORCE" -eq 0 && "$LOCAL" -eq 0 ]]; then
    ok "Phase-1 grid already installed: $existing ($("$existing" -V 2>/dev/null || true))"
    info "Reinstall:  curl … | bash -s -- --force"
    info "Local build: ./scripts/install.sh --local --force"
    verify_install
    exit 0
  fi

  if [[ -n "$existing" ]] && ! is_phase1_binary "$existing"; then
    warn "Found legacy/other grid on PATH: $existing"
    warn "It will be backed up and replaced so 'grid auth' works."
    FORCE=1
  fi

  local built=""
  local tmp_clean=""

  if [[ "$LOCAL" -eq 1 ]]; then
    built="$(build_from_local)"
  elif [[ "$FROM_SOURCE" -eq 1 ]]; then
    built="$(build_from_source_git)"
    tmp_clean=1
  else
    if built="$(try_prebuilt)"; then
      tmp_clean=1
    else
      info "No prebuilt binary (or release not published yet) — building from source…"
      built="$(build_from_source_git)"
      tmp_clean=1
    fi
  fi

  [[ -n "$built" && -x "$built" ]] || die "no binary to install"
  is_phase1_binary "$built" || die "refusing to install non-Phase-1 binary"

  local dest_dir
  dest_dir="$(resolve_bin_dir)"
  replace_conflicting_grids "$built" "$dest_dir"
  ensure_path_export "$dest_dir"

  if [[ "$tmp_clean" -eq 1 ]]; then
    case "$built" in
      /tmp/*|"${TMPDIR:-/tmp}"*) rm -f "$built" 2>/dev/null || true ;;
    esac
  fi

  # Drop a stamp so support can tell how it was installed
  mkdir -p "${HOME}/.grid" 2>/dev/null || true
  if [[ -d "${HOME}/.grid" ]]; then
    cat > "${HOME}/.grid/install-info.txt" <<EOF
installed_at=$(date -u +%Y-%m-%dT%H:%M:%SZ)
binary=$(first_grid_on_path)
version=$($(first_grid_on_path) -V 2>/dev/null || echo unknown)
method=$([ "$LOCAL" -eq 1 ] && echo local || ([ "$FROM_SOURCE" -eq 1 ] && echo source || echo auto))
dest_dir=$dest_dir
EOF
    chmod 600 "${HOME}/.grid/install-info.txt" 2>/dev/null || true
  fi

  verify_install
  print_next
}

main
