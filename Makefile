# Common tasks.  `make help` lists them.

CARGO ?= cargo
# `make deb V=0.3.1` (or `make version V=0.3.1`) sets a new version first.
VERSION := $(or $(V),$(shell sed -n 's/^version = "\(.*\)"/\1/p' crates/app/Cargo.toml | head -1))
DEB := target/pomodoro_$(VERSION)_amd64.deb
# A test instance with its own settings (doesn't touch ~/.config).
DEMO_DIR := $(CURDIR)/target/demo

.PHONY: help check build release run demo test lint fmt deb version docker-test install clean

help: ## Show this list
	@grep -E '^[a-z-]+:.*## ' $(MAKEFILE_LIST) | awk -F':.*## ' '{printf "  make %-12s %s\n", $$1, $$2}'

check: ## Check that Rust and the test tools are installed
	@command -v $(CARGO) >/dev/null || { echo "Rust not found: install it from https://rustup.rs, then open a new terminal (or run: . ~/.cargo/env)"; exit 1; }
	@command -v dbus-daemon >/dev/null || { echo "dbus-daemon not found (the D-Bus test needs it): sudo apt install dbus"; exit 1; }

build: check ## Debug build
	$(CARGO) build

release: check ## Optimized build: target/release/pomodoro
	$(CARGO) build --release

run: check ## Run the debug build with your real settings
	$(CARGO) run

demo: check ## Run a 1-minute timer with scratch settings in target/demo
	mkdir -p $(DEMO_DIR)
	XDG_CONFIG_HOME=$(DEMO_DIR) $(CARGO) run -- --minutes 1

test: check ## Run all tests (unit tests + D-Bus test on a private bus)
	$(CARGO) test

lint: check ## Check formatting and clippy
	$(CARGO) fmt --all --check
	$(CARGO) clippy --all-targets -- -D warnings

fmt: ## Format the code
	$(CARGO) fmt --all

deb: check ## Self-contained .deb (one static binary, no dependencies); V=x.y.z bumps first
	$(if $(V),scripts/bump-version.sh $(V))
	scripts/build-deb.sh

version: ## Set the version of the crates and the .deb: make version V=x.y.z
	@[ -n "$(V)" ] || { echo "Usage: make version V=x.y.z (now $(VERSION))"; exit 1; }
	scripts/bump-version.sh $(V)

docker-test: deb ## Install the .deb in clean Ubuntu containers and run --self-test
	scripts/docker-test.sh

install: deb ## Build and install the Debian package (asks for sudo password)
	sudo apt install ./$(DEB)

clean: ## Delete target/
	$(CARGO) clean
