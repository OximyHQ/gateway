# Deployment

---

## Running the binary

### Local development

```bash
export OPENAI_API_KEY=sk-...
oximy-gateway up
```

The gateway binds to `127.0.0.1:8080` by default — local only. The dashboard
opens in your browser.

### Binding to all interfaces

```bash
oximy-gateway up --host 0.0.0.0 --port 8080
```

When bound to `0.0.0.0`, the startup message displays `127.0.0.1` for local
access. Use your machine's actual IP or DNS name for remote access.

### Headless / server mode

```bash
oximy-gateway up --host 0.0.0.0 --no-open
```

`--no-open` skips the browser launch. Useful on servers, in CI, and in Docker.

### Data directory

The gateway writes its state file (`gateway.json`) to a data directory:

```bash
# Default platform location
oximy-gateway up

# Custom location
oximy-gateway up --dir /var/lib/oximy-gateway

# Via environment variable
export OXIMY_GATEWAY_DIR=/var/lib/oximy-gateway
oximy-gateway up
```

**Platform defaults:**
- Linux: `~/.local/share/oximy-gateway`
- macOS: `~/Library/Application Support/com.oximy.oximy-gateway`
- Windows: `%APPDATA%\oximy\oximy-gateway`

The state file contains hashed key material and gateway configuration. Back it up
if you need to preserve keys across machine migrations. Provider API keys are
encrypted at rest.

---

## Docker

### Run

```bash
docker run -d \
  --name oximy-gateway \
  -p 8080:8080 \
  -e OPENAI_API_KEY=sk-... \
  -e ANTHROPIC_API_KEY=sk-ant-... \
  -e OXIMY_GATEWAY_HOST=0.0.0.0 \
  -v /var/lib/oximy-gateway:/data \
  -e OXIMY_GATEWAY_DIR=/data \
  ghcr.io/oximyhq/gateway:latest \
  oximy-gateway up --no-open
```

The `-v` mount persists the state file (and therefore the admin key) across
container restarts. Without a volume mount, the admin key is regenerated every
time the container starts.

### Getting the admin key from Docker

On first start, check the container logs:

```bash
docker logs oximy-gateway 2>&1 | grep -A3 "First boot"
```

### Docker Compose

```yaml
version: "3.8"
services:
  oximy-gateway:
    image: ghcr.io/oximyhq/gateway:latest
    command: ["oximy-gateway", "up", "--no-open"]
    ports:
      - "8080:8080"
    environment:
      OPENAI_API_KEY: "${OPENAI_API_KEY}"
      ANTHROPIC_API_KEY: "${ANTHROPIC_API_KEY}"
      GEMINI_API_KEY: "${GEMINI_API_KEY}"
      OXIMY_GATEWAY_HOST: "0.0.0.0"
      OXIMY_GATEWAY_DIR: "/data"
    volumes:
      - oximy-data:/data
    healthcheck:
      test: ["CMD", "curl", "-f", "http://localhost:8080/health"]
      interval: 10s
      timeout: 5s
      retries: 3

volumes:
  oximy-data:
```

---

## Reverse proxy / TLS

The gateway does not handle TLS itself. Terminate TLS at a reverse proxy and
proxy plaintext to the gateway.

### nginx

```nginx
server {
    listen 443 ssl;
    server_name gateway.example.com;

    ssl_certificate /etc/ssl/certs/example.com.crt;
    ssl_certificate_key /etc/ssl/private/example.com.key;

    location / {
        proxy_pass http://127.0.0.1:8080;
        proxy_http_version 1.1;
        # Required for SSE streaming
        proxy_set_header Connection "";
        proxy_buffering off;
        proxy_read_timeout 300s;
        # Forward client IP
        proxy_set_header X-Forwarded-For $remote_addr;
        proxy_set_header X-Forwarded-Proto $scheme;
    }
}
```

The `proxy_buffering off` and `proxy_read_timeout 300s` settings are important for
streaming responses — without them, nginx will buffer SSE chunks and clients will
see delayed output.

### Caddy

```
gateway.example.com {
    reverse_proxy 127.0.0.1:8080 {
        flush_interval -1
    }
}
```

`flush_interval -1` disables buffering for SSE.

### Cloudflare Tunnel

Cloudflare Tunnel works out of the box for non-streaming requests. For SSE
streaming, you need Cloudflare's `no-buffering` response header. Add this to your
gateway's startup environment:

```bash
# Cloudflare strips buffering when it sees this header
# Set it in nginx / Caddy above instead of in the gateway
```

The gateway itself does not set special streaming headers for proxies; set them at
the proxy layer.

---

## Systemd service (Linux)

Create `/etc/systemd/system/oximy-gateway.service`:

```ini
[Unit]
Description=Oximy Gateway
After=network.target

[Service]
Type=simple
User=oximy
Group=oximy
ExecStart=/usr/local/bin/oximy-gateway up --host 0.0.0.0 --no-open
Restart=on-failure
RestartSec=5s

# Provider API keys — store in /etc/oximy-gateway/env
EnvironmentFile=/etc/oximy-gateway/env

# Data directory
Environment=OXIMY_GATEWAY_DIR=/var/lib/oximy-gateway

# Logging
StandardOutput=journal
StandardError=journal
SyslogIdentifier=oximy-gateway

[Install]
WantedBy=multi-user.target
```

Create the env file at `/etc/oximy-gateway/env` (chmod 600):

```bash
OPENAI_API_KEY=sk-...
ANTHROPIC_API_KEY=sk-ant-...
GEMINI_API_KEY=AIza...
```

Enable and start:

```bash
sudo useradd -r -s /bin/false oximy
sudo mkdir -p /var/lib/oximy-gateway
sudo chown oximy:oximy /var/lib/oximy-gateway
sudo systemctl daemon-reload
sudo systemctl enable oximy-gateway
sudo systemctl start oximy-gateway
sudo journalctl -u oximy-gateway -f
```

---

## Persistence

### What is persisted

The `gateway.json` state file contains:
- Hashed virtual key material (secrets are hashed; the hash cannot be reversed)
- Key metadata (name, budget, allowlists, expiry)
- Accumulated spend per key (used to enforce budgets across restarts)
- Registered MCP servers (if added via the dashboard or CLI)

Provider API keys are stored encrypted in the state file. The encryption key is
derived from a master key that is generated on first boot and stored separately.

### Backup

```bash
# Stop the gateway, copy the data directory, restart
cp -r /var/lib/oximy-gateway /backup/oximy-gateway-$(date +%Y%m%d)
```

Or use a volume snapshot if running in Docker/Kubernetes.

### Losing the state file

If the state file is lost:
- All virtual keys are gone. Keys in the old file no longer work.
- The admin key is gone. You will need to create a new one on first boot.
- Accumulated spend resets. Budget enforcement starts fresh.
- MCP server registrations are gone.

Config-file-managed settings (from `oximy-gateway.json`) can be re-applied after
recovery.

---

## Scaling

The gateway is stateless at the HTTP layer. Multiple instances can run behind a
load balancer with a shared data directory (or a shared Postgres database in
Postgres mode). Budget enforcement requires all instances to share the same ledger —
for multi-instance setups, use the Postgres backend and a Redis gossip layer for
in-memory budget state synchronization (planned, P4 / P5).

For single-instance deployments, the SQLite default is production-ready.

---

## Resource requirements

| Resource | Typical (single instance) |
|---|---|
| Binary size | < 50 MB |
| RAM (idle) | < 100 MB |
| RAM (under load) | 150–300 MB depending on request volume |
| Cold start time | ~100 ms |
| CPU | 1–4 vCPUs for 5–10k RPS |

The gateway is designed to have minimal resource overhead. Provider latency
dominates total request latency; gateway overhead is in the sub-1ms range.

---

## Security hardening

- **TLS at the proxy layer** — never expose port 8080 directly to the internet
- **Firewall the metrics port** — `/metrics` is authenticated, but also restrict
  it at the network level if possible
- **Use scoped keys** — never give production applications the admin key
- **Restrict `--host`** — bind to `127.0.0.1` and let the reverse proxy forward;
  only use `0.0.0.0` when you have a network-level firewall in place
- **Protect the data directory** — `chmod 700` the data dir; it contains encrypted
  key material
- **Rotate provider keys** — if a provider key is compromised, revoke it at the
  provider and update the env var; restart the gateway to pick up the new key
