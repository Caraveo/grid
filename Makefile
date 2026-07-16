# GRID — Phase 1 useful mining CLI
# https://github.com/Caraveo/grid

.PHONY: help build release install install-system uninstall check auth-help clean

CARGO ?= cargo
PREFIX ?= $(HOME)/.local/bin
BIN    := target/release/grid

help:
	@echo "GRID Makefile"
	@echo ""
	@echo "  make build            Debug build"
	@echo "  make release          Optimized release binary"
	@echo "  make install          Build release → $(PREFIX)/grid (+ fix legacy)"
	@echo "  make install-system   Install to /usr/local/bin"
	@echo "  make uninstall        Remove Phase-1 binaries from standard paths"
	@echo "  make check            cargo check"
	@echo "  make auth-help        Confirm installed grid has auth"
	@echo "  make clean            cargo clean"
	@echo ""
	@echo "One-liner (any machine):"
	@echo "  curl -fsSL https://raw.githubusercontent.com/Caraveo/grid/master/scripts/install.sh | bash"

build:
	$(CARGO) build

release:
	$(CARGO) build --release

install: release
	@bash scripts/install.sh --local --force --prefix="$(PREFIX)"

install-system: release
	@bash scripts/install.sh --local --force --system

uninstall:
	@bash scripts/install.sh --uninstall

check:
	$(CARGO) check

auth-help:
	@hash -r 2>/dev/null || true
	@command -v grid >/dev/null || (echo "grid not on PATH"; exit 1)
	@grid auth --help
	@echo ""
	@which grid
	@grid -V

clean:
	$(CARGO) clean
