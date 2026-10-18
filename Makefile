# Shortcuts for common development tasks. Each target wraps the npm/cargo command listed in
# CLAUDE.md, so CI and `make` run the same thing. Run `make help` for the list.

.DEFAULT_GOAL := help
.PHONY: help install dev build build-debug lint fmt test check types sidecar mcp clean

help: ## Show this help
	@awk 'BEGIN {FS = ":.*## "} /^[a-z-]+:.*## / {printf "  \033[36m%-12s\033[0m %s\n", $$1, $$2}' $(MAKEFILE_LIST)

node_modules: package.json package-lock.json
	npm install
	@touch node_modules

install: node_modules ## Install frontend dependencies

dev: node_modules ## Run the app with hot reload (builds the MCP sidecar first)
	npm run tauri dev

build: node_modules ## Build release bundles for this platform
	npm run tauri build

build-debug: node_modules ## Build a debug app without installers (faster)
	npm run tauri build -- --debug --no-bundle

lint: node_modules sidecar ## Check formatting and run clippy, ESLint and TypeScript
	cargo fmt --all --check
	cargo clippy --workspace --all-targets -- -D warnings
	npm run format:check
	npm run lint
	npm run typecheck

fmt: node_modules ## Format Rust and frontend code
	cargo fmt --all
	npm run format

test: node_modules sidecar ## Run Rust and frontend tests
	cargo test --workspace
	npm test

check: node_modules ## The full gate: lint, tests and cargo-deny
	npm run check

types: ## Regenerate TypeScript types from Rust (ts-rs)
	npm run gen:types

sidecar: ## Build the debug boxdeck-mcp sidecar that tauri-build expects
	node scripts/build-sidecar.mjs --debug

mcp: ## Run boxdeck-mcp over stdio (for trying it with an MCP client)
	cargo run -q -p boxdeck-mcp

clean: ## Remove build output (Rust target, frontend dist, sidecars)
	cargo clean
	rm -rf dist src-tauri/binaries
