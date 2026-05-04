# Blackhole development tasks. Run `make help` for the menu.
#
# Tool prerequisites (install once):
#   curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
#   rustup toolchain install stable --component clippy
#   rustup toolchain install nightly --component rustfmt
#   cargo install cargo-deny
#   brew install act      # for `make act-*` targets
#   # Docker Desktop or `colima start` for `make act-*` and `make compose-up`.

ACT ?= act
# We invoke rustup's cargo by absolute path so the Makefile works even if
# rustup isn't on $PATH (e.g., for Homebrew rust users who installed rustup
# with --no-modify-path). Toolchain selection is explicit per-target.
RUSTUP_CARGO ?= $(HOME)/.cargo/bin/cargo
STABLE  := $(RUSTUP_CARGO) +stable
NIGHTLY := $(RUSTUP_CARGO) +nightly

# Network-dependent upstream tests skipped by default; mirrors push.yml.
TEST_SKIPS := --skip test_crowded --skip test_connect_with --skip test_send_many \
              --skip test_wrong_code --skip test_file_rust2rust

.PHONY: help build test test-all fmt fmt-check clippy clippy-strict deny ci \
        act act-fmt act-clippy act-test act-deny \
        smoke compose-up compose-down compose-logs mailbox-run transit-run

help: ## Show this help.
	@awk 'BEGIN {FS = ":.*?## "} /^[a-zA-Z_-]+:.*?##/ {printf "  \033[36m%-16s\033[0m %s\n", $$1, $$2}' $(MAKEFILE_LIST)

# --- local checks ---

build: ## Build the workspace.
	$(STABLE) build --workspace

test: ## Run unit tests, skipping upstream network-dependent tests.
	$(STABLE) test --workspace -- $(TEST_SKIPS)

test-all: ## Run every test (requires reachable rendezvous server for upstream tests).
	$(STABLE) test --workspace

fmt: ## Apply nightly rustfmt across the workspace.
	$(NIGHTLY) fmt --all

fmt-check: ## Verify the workspace is rustfmt-clean (matches CI).
	$(NIGHTLY) fmt --all -- --check

clippy: ## Run clippy (matches CI scope: default members, all features).
	$(STABLE) clippy --all-features

clippy-strict: ## Stricter local clippy: whole workspace, warnings as errors.
	RUSTFLAGS="-D warnings" $(STABLE) clippy --workspace --all-features

deny: ## Run cargo-deny (advisories, licenses, bans, sources).
	$(STABLE) deny check

# --- CI parity ---
# `make ci` runs the same checks CI does, but natively — no Docker or qemu.
# Faster, more reliable. The `act-*` targets below are an opt-in heavier path
# that runs the actual workflow inside Docker; useful when modifying push.yml.

ci: fmt-check clippy deny test ## Mirror the cheap CI checks natively (no Docker).

# act-based runners. Known issue: Swatinem/rust-cache (pulled in by
# actions-rust-lang/setup-rust-toolchain) crashes on missing `node` in PATH
# when running through act + qemu emulation on Apple Silicon. Use these only
# when you need to validate workflow-file changes themselves.
act: act-fmt act-clippy act-deny act-test ## Run the cheap CI jobs in local Docker via act.

act-fmt: ## Run the Cargo Format job under act.
	$(ACT) -j formatting --workflows .github/workflows/push.yml

act-clippy: ## Run the Clippy job under act.
	$(ACT) -j clippy --workflows .github/workflows/push.yml

act-test: ## Run the Test job under act (matrix; very slow under qemu).
	$(ACT) -j test --workflows .github/workflows/push.yml

act-deny: ## Run the Cargo deny job under act.
	$(ACT) -j cargo-deny --workflows .github/workflows/push.yml

# --- end-to-end ---

smoke: ## Local end-to-end smoke test: mailbox + transit + CLI roundtrip.
	@scripts/smoke.sh

compose-up: ## docker compose up the full stack (postgres + mailbox + transit).
	docker compose up -d --build

compose-down: ## Tear down the compose stack (preserves the postgres volume).
	docker compose down

compose-logs: ## Tail logs from the composed services.
	docker compose logs -f

mailbox-run: ## Run blackhole-mailbox locally on 127.0.0.1:4000 (in-memory store).
	$(STABLE) run -p blackhole-mailbox -- --listen 127.0.0.1:4000

transit-run: ## Run blackhole-transit locally on 127.0.0.1:4001.
	$(STABLE) run -p blackhole-transit -- --listen 127.0.0.1:4001
