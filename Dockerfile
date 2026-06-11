# Multi-stage Dockerfile for oximy-gateway.
#
# Stage 1 (builder): compiles the release binary with the pinned Rust toolchain.
# Stage 2 (runtime): minimal debian-slim image — only the binary + CA certs.
#
# Build:  docker build -t oximy-gateway .
# Run:    docker run -p 8080:8080 \
#           -e OPENAI_API_KEY=sk-... \
#           -v /my/data:/data \
#           oximy-gateway

# ── Stage 1: Build ─────────────────────────────────────────────────────────────
FROM rust:1.92-slim-bookworm AS builder

WORKDIR /build

# Install system dependencies needed for the build
# (openssl-dev for reqwest with native-tls; we use rustls-tls so this is minimal)
RUN apt-get update -qq && apt-get install -y --no-install-recommends \
    pkg-config \
    libssl-dev \
    && rm -rf /var/lib/apt/lists/*

# Copy workspace manifests first for better layer caching.
# We copy Cargo.toml + Cargo.lock before source so dependency fetching is cached
# independently of source changes.
COPY Cargo.toml Cargo.lock rust-toolchain.toml ./
COPY crates/gateway-cache/Cargo.toml     crates/gateway-cache/Cargo.toml
COPY crates/gateway-config/Cargo.toml    crates/gateway-config/Cargo.toml
COPY crates/gateway-control/Cargo.toml   crates/gateway-control/Cargo.toml
COPY crates/gateway-dash/Cargo.toml      crates/gateway-dash/Cargo.toml
COPY crates/gateway-guard/Cargo.toml     crates/gateway-guard/Cargo.toml
COPY crates/gateway-llm/Cargo.toml       crates/gateway-llm/Cargo.toml
COPY crates/gateway-mcp/Cargo.toml       crates/gateway-mcp/Cargo.toml
COPY crates/gateway-route/Cargo.toml     crates/gateway-route/Cargo.toml
COPY crates/gateway-spine/Cargo.toml     crates/gateway-spine/Cargo.toml
COPY crates/gateway-telemetry/Cargo.toml crates/gateway-telemetry/Cargo.toml
COPY crates/oximy-gateway/Cargo.toml     crates/oximy-gateway/Cargo.toml

# Create stub lib/main files so `cargo fetch` resolves all dependencies
# without needing the full source tree.
RUN find crates -name "Cargo.toml" | while read f; do \
      dir=$(dirname "$f"); \
      mkdir -p "$dir/src"; \
      # Only create stub if no src exists yet
      [ -f "$dir/src/lib.rs" ]  || echo "// stub" > "$dir/src/lib.rs"; \
      [ -f "$dir/src/main.rs" ] || echo "fn main() {}" > "$dir/src/main.rs"; \
    done

RUN cargo fetch --locked

# Now copy the real source
COPY crates/ crates/

# Build the release binary.
# The workspace Cargo.toml already sets: lto=thin, codegen-units=1, strip=true, panic=abort
RUN cargo build --release --bin oximy-gateway --locked \
    && strip target/release/oximy-gateway 2>/dev/null || true

# ── Stage 2: Runtime ───────────────────────────────────────────────────────────
FROM debian:bookworm-slim AS runtime

# CA certificates (needed for TLS connections to LLM provider APIs)
# wget is included for the HEALTHCHECK (curl is not available in debian-slim by default)
RUN apt-get update -qq && apt-get install -y --no-install-recommends \
    ca-certificates \
    wget \
    && rm -rf /var/lib/apt/lists/* \
    && update-ca-certificates

# Non-root user for security
RUN groupadd --gid 10001 oximy \
    && useradd  --uid 10001 --gid oximy --shell /usr/sbin/nologin --no-create-home oximy

# Default data directory — users bind-mount a host volume here
ENV OXIMY_GATEWAY_DIR=/data
VOLUME ["/data"]

# Port the gateway listens on
EXPOSE 8080

# Copy the stripped binary from the builder stage
COPY --from=builder /build/target/release/oximy-gateway /usr/local/bin/oximy-gateway

# Ensure the data directory exists and is owned by the runtime user
RUN mkdir -p /data && chown oximy:oximy /data

USER oximy

# Provider API keys are passed at runtime via environment variables:
#   OPENAI_API_KEY, ANTHROPIC_API_KEY, GEMINI_API_KEY, OPENROUTER_API_KEY
# Do NOT bake secrets into the image.
#
# Optional env vars:
#   OXIMY_GATEWAY_PORT (default 8080)
#   OXIMY_GATEWAY_HOST (default 0.0.0.0 in container)
#   OXIMY_ROUTES       (JSON route overrides)
#   OXIMY_MCP_SERVERS  (JSON array of upstream MCP servers)

ENTRYPOINT ["/usr/local/bin/oximy-gateway"]
CMD ["up", "--host", "0.0.0.0", "--port", "8080", "--no-open"]

# Health check — polls /health which requires no auth
HEALTHCHECK --interval=30s --timeout=5s --start-period=10s --retries=3 \
    CMD ["wget", "-q", "-O", "/dev/null", "http://localhost:8080/health"]
