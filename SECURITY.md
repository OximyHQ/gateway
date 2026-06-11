# Security Policy

Oximy Gateway holds provider API keys and brokers credentials — **it is the vault
door.** Security is a first-class product property, not an afterthought.

## Reporting a vulnerability

Email **security@oximy.com** with details and reproduction steps. Please do not
open a public issue for undisclosed vulnerabilities. We aim to acknowledge within
2 business days and to ship a fix or mitigation on a coordinated-disclosure
timeline.

## Security posture (design commitments)

- **Auth by default** — admin API, dashboard, and `/metrics` require auth out of
  the box. No permissive defaults.
- **Secrets never leak** — provider keys encrypted at rest; outbound credentials
  injected server-side and never returned to clients; secrets never logged;
  `--block-secrets` default-on for MCP tool payloads.
- **Supply chain** — release artifacts are cosign-signed with SBOM + provenance;
  publish credentials are isolated.
- **Isolation** — community plugins run in a WASM sandbox; a panic must never
  crash the API server.
- **No SSRF** — passthrough and custom-host transports enforce allowlists and deny
  private/link-local ranges.

## Supported versions

Pre-1.0: only the latest release receives security fixes. A formal support policy
will accompany the 1.0 release.
