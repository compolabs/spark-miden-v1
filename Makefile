.DEFAULT_GOAL := help

.PHONY: help
help: ## Show description of all commands
	@grep -E '^[a-zA-Z_-]+:.*?## .*$$' $(MAKEFILE_LIST) | sort | awk 'BEGIN {FS = ":.*?## "}; {printf "\033[36m%-30s\033[0m %s\n", $$1, $$2}'

# --- Variables -----------------------------------------------------------------------------------

FEATURES_INTEGRATION_TESTING="integration"
FEATURES_CLI="testing, concurrent"
NODE_FEATURES_TESTING="testing"
WARNINGS=RUSTDOCFLAGS="-D warnings"
NODE_BRANCH="main"

# --- Linting -------------------------------------------------------------------------------------

.PHONY: clippy
 clippy: ## Runs Clippy with configs
	cargo +nightly clippy --workspace --all-targets --features $(FEATURES_CLI) -- -D warnings

.PHONY: fix
fix: ## Runs Fix with configs
	cargo +nightly fix --allow-staged --allow-dirty --all-targets --features $(FEATURES_CLI)

.PHONY: format
format: ## Runs format using nightly toolchain
	cargo +nightly fmt --all

.PHONY: format-check
format-check: ## Runs format using nightly toolchain but only in check mode
	cargo +nightly fmt --all --check

.PHONY: lint
lint: format fix clippy ## Runs all linting tasks at once (clippy, fixing, formatting)

# --- Testing -------------------------------------------------------------------------------------

.PHONY: test
test: ## Run tests
	cargo nextest run --release --workspace --features $(FEATURES_CLI)

# --- Integration testing -------------------------------------------------------------------------

.PHONY: integration-test
integration-test: build ## Run integration tests
	cargo nextest run --release --test=integration --features $(FEATURES_INTEGRATION_TESTING) --no-default-features

.PHONY: integration-test-full
integration-test-full: build ## Run the integration test binary with ignored tests included
	cargo nextest run --release --test=integration --features $(FEATURES_INTEGRATION_TESTING)
	cargo nextest run --release --test=integration --features $(FEATURES_INTEGRATION_TESTING) --run-ignored ignored-only -- test_import_genesis_accounts_can_be_used_for_transactions

.PHONY: kill-node
kill-node: ## Kill node process
	pkill miden-node || echo 'process not running'

.PHONY: clean-node
clean-node: ## Clean node directory
	rm -rf miden-node

.PHONY: node
node: ## Setup node directory
	if [ -d miden-node ]; then cd miden-node ; else git clone https://github.com/0xPolygonMiden/miden-node.git && cd miden-node; fi
	cd miden-node && git checkout $(NODE_BRANCH) && git pull origin $(NODE_BRANCH) && cargo update
	cd miden-node && rm -rf miden-store.sqlite3*
	cd miden-node && cargo run --bin miden-node --features $(NODE_FEATURES_TESTING) -- make-genesis --inputs-path ../tests/config/genesis.toml --force

.PHONY: start-node
start-node: ## Run node. This requires the node repo to be present at `miden-node`
	cd miden-node && cargo run --bin miden-node --features $(NODE_FEATURES_TESTING) -- start --config ../tests/config/miden-node.toml node

