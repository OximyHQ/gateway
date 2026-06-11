# Guardrails

Guardrails are filters that inspect request text (prompts, tool inputs, tool
outputs) and either block, mask, flag, or simply observe. They run on the shared
governance spine, so they apply to both LLM calls and MCP tool calls.

---

## What is checked by default

On first boot, Oximy Gateway enables two guardrails in `Enforce` mode:

1. **Secrets detection** — blocks any request containing recognizable API key or
   credential patterns (see the full list below)
2. **PII detection** — by default in `ObserveOnly` mode (logs but does not block)

These defaults protect against the most common accidental leaks. You can change
the mode or add more guardrails via configuration.

---

## Built-in guardrails

### Secrets detection (`secrets`)

Blocks requests that contain known provider secret formats:

| Pattern | Example |
|---|---|
| OpenAI API key | `sk-` followed by 20+ alphanumeric/punctuation characters |
| AWS access key ID | `AKIA` followed by 16 uppercase alphanumeric characters |
| GitHub personal access token | `ghp_` prefix |
| Slack bot token | `xoxb-` prefix |
| Slack user token | `xoxp-` prefix |
| GitLab personal access token | `glpat-` prefix |

**Why this matters for agents.** An agent that echoes a prompt containing an API
key back to an LLM leaks that key into provider logs and model training pipelines.
The secrets guardrail catches this before the request leaves your network.

When a secret is detected in `Enforce` mode:

```http
HTTP/1.1 403 Forbidden
Content-Type: application/json

{
  "error": {
    "type": "guardrail_violation",
    "message": "Request blocked: detected OpenAI API key in prompt",
    "code": "guardrail_block"
  }
}
```

### PII detection (`pii`)

Detects common personally identifiable information patterns:
- Email addresses
- Phone numbers (various formats)
- Social Security Numbers
- Credit card numbers

In `ObserveOnly` mode (default), PII is logged but the request proceeds. Switch to
`Enforce` to block, or `Mask` to redact before forwarding.

### Keyword banning (`keyword`)

Block requests containing specific words or phrases:

```json
{
  "type": "keyword",
  "mode": "enforce",
  "keywords": ["internal-project-codename", "competitor-name"]
}
```

### Regex denylist (`regex_deny`)

Block requests matching a regular expression:

```json
{
  "type": "regex_deny",
  "mode": "enforce",
  "pattern": "\\b(ssn|social security)\\b",
  "label": "SSN reference"
}
```

### JSON schema validation (`schema`)

Validate that a request or response body conforms to a JSON schema:

```json
{
  "type": "schema",
  "mode": "enforce",
  "schema": {
    "type": "object",
    "required": ["task"],
    "properties": {
      "task": {"type": "string", "maxLength": 500}
    }
  }
}
```

### External webhook (`webhook`)

Forward the request text to an external HTTP content moderation endpoint
(Lakera Guard, Azure Content Safety, custom classifiers). The guardrail expects
a JSON response with a `verdict` field:

```json
{
  "type": "webhook",
  "mode": "enforce",
  "url": "https://guard.example.com/check",
  "timeout_ms": 200
}
```

External webhooks run asynchronously off the hot path when in `ObserveOnly` mode.
In `Enforce` mode they add latency; set a generous `timeout_ms` or use
`ObserveOnly` for high-throughput paths.

---

## Enforcement modes

Each guardrail has an enforcement mode that controls what happens when it fires:

| Mode | Effect |
|---|---|
| `enforce` | `Block` verdicts short-circuit the request (403); `Mask` verdicts redact the text before forwarding to subsequent guardrails |
| `observe_only` | Record what the guardrail *would* have done; never block or mutate the request |
| `dry_run` | Simulate the full chain — record hypothetical outcomes without acting on any of them |

### Verdict types

| Verdict | Description |
|---|---|
| `Allow` | Request is clean; proceed normally |
| `Block` | Request must be rejected; `reason` is surfaced to the caller |
| `Mask` | Sensitive content found; a `redacted_text` replacement is used for subsequent guardrails |
| `Flag` | Noteworthy but not blocking; annotate and continue |

---

## Configuring guardrails

### Via `oximy-gateway.json`

```json
{
  "guardrails": [
    {
      "id": "default",
      "apply_to": "all_keys",
      "rules": [
        {
          "type": "secrets",
          "mode": "enforce"
        },
        {
          "type": "pii",
          "mode": "mask"
        },
        {
          "type": "keyword",
          "mode": "enforce",
          "keywords": ["confidential", "top-secret"]
        }
      ]
    }
  ]
}
```

### Attaching guardrails to specific keys

```json
{
  "guardrails": [
    {
      "id": "strict",
      "apply_to": ["key_external_app"],
      "rules": [
        {"type": "secrets", "mode": "enforce"},
        {"type": "pii", "mode": "enforce"},
        {"type": "keyword", "mode": "enforce", "keywords": ["ssn", "password"]}
      ]
    },
    {
      "id": "relaxed",
      "apply_to": ["key_internal_admin"],
      "rules": [
        {"type": "secrets", "mode": "observe_only"}
      ]
    }
  ]
}
```

---

## Pipeline stages

Guardrails run at four stages in the request lifecycle:

| Stage | When | What is inspected |
|---|---|---|
| `pre_request` | Before forwarding to provider | The user's prompt / messages |
| `post_response` | After receiving from provider | The model's response text |
| `pre_tool_call` | Before dispatching an MCP tool | Tool name and arguments |
| `post_tool_result` | After receiving MCP tool result | Tool output |

By default, guardrails run at `pre_request`. Specify `stages` to target other
points:

```json
{
  "type": "secrets",
  "mode": "enforce",
  "stages": ["pre_request", "post_response", "pre_tool_call", "post_tool_result"]
}
```

---

## Dry-run mode

Dry-run lets you test a guardrail configuration without affecting any real
requests. All requests flow through; the guardrail records what it *would* have
done.

```json
{
  "type": "pii",
  "mode": "dry_run"
}
```

Check the request log in the dashboard to see the simulated verdicts. Switch to
`enforce` when you are confident in the configuration.

---

## Audit trail

Every guardrail verdict — including `Allow` (when in `observe_only` or `dry_run`
mode) — is recorded in the spine audit log with:

- Timestamp
- Key ID
- Guardrail ID and type
- Stage
- Verdict and reason
- Whether the verdict was enforced or observed

Audit events appear in the dashboard under **Requests > Guardrail verdicts** and
are exported to the telemetry store.

---

## Performance

Guardrails run synchronously on the hot path in `Enforce` mode. Built-in
guardrails (secrets, PII, keyword, regex) are deterministic and fast (sub-
millisecond). External webhooks add round-trip latency.

For high-throughput paths where you want observability without added latency, use
`observe_only` — guardrail evaluation is still logged but the result never holds
up the response.
