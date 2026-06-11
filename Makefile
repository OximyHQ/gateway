# Makefile for oximy-gateway
# Common developer tasks. Requires: cargo, docker (optional), bash/sh.
#
# Usage:
#   make build          — release binary (local, current platform)
#   make run            — build + run (auto-opens browser)
#   make run-headless   — build + run without browser, 0.0.0.0
#   make test           — cargo test (all features, workspace)
#   make check          — fmt + clippy + test (mirrors CI)
#   make docker-build   — build the Docker image locally
#   make docker-run     — run the Docker image on port 8080
#   make dist-plan      — preview what cargo-dist would release
#   make dist-build     — local cargo-dist build (all platforms; slow)
#   make clean          — cargo clean

BINARY      := oximy-gateway
CARGO       := cargo
IMAGE       := ghcr.io/oximyhq/gateway
IMAGE_TAG   := dev

.PHONY: all build run run-headless test check fmt clippy \
        docker-build docker-run dist-plan dist-build clean help

all: build

## Build the release binary for the current platform
build:
	$(CARGO) build --release --bin $(BINARY)
	@echo ""
	@echo "  Binary: target/release/$(BINARY)"

## Build + run the gateway (opens browser on first run)
run: build
	./target/release/$(BINARY) up

## Build + run headlessly on 0.0.0.0:8080 (server mode)
run-headless: build
	./target/release/$(BINARY) up --host 0.0.0.0 --port 8080 --no-open

## Run tests (all features, workspace)
test:
	$(CARGO) test --all-features --workspace

## Run fmt check
fmt:
	$(CARGO) fmt --all --check

## Run clippy (warnings = errors)
clippy:
	$(CARGO) clippy --all-targets --all-features -- -D warnings

## Full CI check: fmt + clippy + test
check: fmt clippy test

## Build Docker image locally
docker-build:
	docker build -t $(IMAGE):$(IMAGE_TAG) .
	@echo ""
	@echo "  Image: $(IMAGE):$(IMAGE_TAG)"

## Run the Docker image (set OPENAI_API_KEY etc. in env or pass -e flags)
docker-run: docker-build
	docker run --rm -it \
		-p 8080:8080 \
		-v "$(HOME)/.local/share/oximy-gateway-docker:/data" \
		-e OPENAI_API_KEY=$${OPENAI_API_KEY:-} \
		-e ANTHROPIC_API_KEY=$${ANTHROPIC_API_KEY:-} \
		-e GEMINI_API_KEY=$${GEMINI_API_KEY:-} \
		-e OPENROUTER_API_KEY=$${OPENROUTER_API_KEY:-} \
		$(IMAGE):$(IMAGE_TAG)

## Preview what cargo-dist would release (dry run)
dist-plan:
	cargo dist plan

## Build all release artifacts locally via cargo-dist (slow; needs dist installed)
dist-build:
	cargo dist build

## Install cargo-dist (needed for dist-plan / dist-build)
dist-install:
	curl --proto '=https' --tlsv1.2 -LsSf \
		https://github.com/axodotdev/cargo-dist/releases/download/v0.28.0/cargo-dist-installer.sh | sh

## Syntax-check install.sh
check-installer:
	bash -n install.sh
	@echo "install.sh syntax OK"

## cargo clean
clean:
	$(CARGO) clean

## Show this help
help:
	@grep -E '^##' Makefile | sed 's/^## /  /'
