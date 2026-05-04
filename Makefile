# Blackhole development tasks. Run `make help` for the menu.
#
# Tool prerequisites (install once):
#   - rustup + nightly + rustfmt:  curl https://sh.rustup.rs | sh && rustup toolchain install nightly --component rustfmt
#   - cargo-deny:                  cargo install cargo-deny
#   - act (CI locally via Docker): brew install act
#   - Docker:                      Docker Desktop or `colima start`

CARGO ?= cargo
ACT   ?= act
# rustfmt's unstable features (imports_granularity, match_block_trailing_comma)
# require nightly. We invoke rustup's cargo by absolute path so that fmt works
# even if rustup isn't on $PATH (Homebrew rust users).
RUSTUP_CARGO ?= $(HOME)/.cargo/bin/cargo

# Network-dependent upstream tests skipped by default; mirrors push.yml.
TEST_SKIPS := --skip test_crowded --skip test_connect_with --skip test_send_many \
              --skip test_wrong_code --skip test_file_rust2rust

.PHONY: help build test test-all fmt fmt-check clippy deny ci ci-fmt ci-clippy ci-test \
        ci-deny smoke compose-up compose-down compose-logs mailbox-run transit-run

help: ## Show this help.
	@awk 'BEGIN {FS = ":.*?## "} /^[a-zA-Z_-]+:.*?##/ {printf "  \033[36m%-16s\033[0m %s\n", $$1, $$2}' $(MAKEFILE_LIST)

# --- local checks ---

build: ## Build the workspace.
	$(CARGO) build --workspace

test: ## Run unit tests, skipping upstream network-dependent tests.
	$(CARGO) test --workspace -- $(TEST_SKIPS)

test-all: ## Run every test (requires reachable rendezvous server for upstream tests).
	$(CARGO) test --workspace

fmt: ## Apply nightly rustfmt across the workspace.
	$(RUSTUP_CARGO) +nightly fmt --all

fmt-check: ## Verify the workspace is rustfmt-clean (matches CI).
	$(RUSTUP_CARGO) +nightly fmt --all -- --check

clippy: ## Run clippy with warnings as errors.
	$(CARGO) clippy --workspace --all-features -- -D warnings

deny: ## Run cargo-deny (advisories, licenses, bans, sources).
	$(CARGO) deny check

# --- CI mirroring via act ---

ci: ci-fmt ci-clippy ci-deny ci-test ## Run the cheap CI jobs locally via act.

ci-fmt: ## Run the Cargo Format CI job locally.
	$(ACT) -j formatting --workflows .github/workflows/push.yml

ci-clippy: ## Run the Clippy CI job locally.
	$(ACT) -j clippy --workflows .github/workflows/push.yml

ci-test: ## Run the Test CI job locally (matrix; will be slow).
	$(ACT) -j test --workflows .github/workflows/push.yml

ci-deny: ## Run the Cargo deny CI job locally.
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
	$(CARGO) run -p blackhole-mailbox -- --listen 127.0.0.1:4000

transit-run: ## Run blackhole-transit locally on 127.0.0.1:4001.
	$(CARGO) run -p blackhole-transit -- --listen 127.0.0.1:4001
