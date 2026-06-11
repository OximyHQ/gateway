# Keys & Budgets

Virtual keys are the primary access-control and cost-governance primitive in Oximy
Gateway. Every API request requires a bearer token that maps to a virtual key.
The gateway enforces the key's budget, rate limits, and model allowlist before
forwarding anything to a provider.

---

## Concepts

**Virtual key** — a bearer token (`ogw_...`) that wraps policy:
- USD spend budget (hard limit)
- Rate limits (requests per minute, tokens per minute)
- Model allowlist (which models the key is allowed to call)
- Expiry date (optional)

**Admin key** — created on first boot; has an unlimited budget. Use it to
manage the gateway and create scoped keys for teammates and applications.

**Budget** — a USD spend ceiling. The gateway tracks exact cost for every
request (including cached tokens and aborted streaming requests). When a key
reaches its budget, the gateway returns `429` with a budget-exceeded error
**before** making any upstream call. Budgets are enforced atomically under
concurrency — no overspend is possible.

---

## The `keys` CLI

### Create a key

```bash
oximy-gateway keys create --name "alice-dev"
```

With a $10 budget:

```bash
oximy-gateway keys create --name "alice-dev" --budget 10.00
```

With a model allowlist:

```bash
oximy-gateway keys create \
  --name "ci-pipeline" \
  --budget 5.00 \
  --models gpt-4o-mini
```

With rate limits (60 RPM, 100k TPM):

```bash
oximy-gateway keys create \
  --name "high-volume" \
  --budget 100.00 \
  --rpm 60 \
  --tpm 100000
```

With an expiry date:

```bash
oximy-gateway keys create \
  --name "temp-demo" \
  --budget 1.00 \
  --expires 2026-12-31
```

All options together:

```bash
oximy-gateway keys create \
  --name "alice-dev" \
  --budget 10.00 \
  --models "gpt-4o,gpt-4o-mini,claude-3-5-sonnet-20241022" \
  --rpm 120 \
  --tpm 500000 \
  --expires 2026-09-01
```

**Output** (the secret is shown once):

```
Key created.
  ID:      key_abc123def456
  Name:    alice-dev
  Secret:  ogw_xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx
  Budget:  $10.00
  Models:  gpt-4o, gpt-4o-mini
  RPM:     120
  TPM:     500000
  Expires: 2026-09-01
```

Save the secret. It is stored as a hash and cannot be recovered.

### List keys

```bash
oximy-gateway keys list
```

```
ID              NAME          SPENT       BUDGET    MODELS            STATUS
key_abc123...   alice-dev     $3.24       $10.00    gpt-4o, ...       active
key_def456...   ci-pipeline   $0.87       $5.00     gpt-4o-mini       active
```

```bash
# JSON output for scripting
oximy-gateway keys list --json
```

### Revoke a key

```bash
oximy-gateway keys revoke key_abc123def456
```

The key is immediately rejected on any subsequent request. Revocation is
permanent — create a new key if access needs to be restored.

---

## Budget enforcement

**Fail-closed** — the gateway reserves the estimated cost of a request
**before** forwarding it to the provider. If the reservation would exceed the
remaining budget, the request is rejected with:

```http
HTTP/1.1 429 Too Many Requests
Content-Type: application/json

{
  "error": {
    "type": "budget_exceeded",
    "message": "Key key_abc123def456 has exhausted its budget ($10.00)",
    "code": "budget_exceeded"
  }
}
```

After the response is received, the gateway commits the **actual** cost from
provider-reported usage (true-up) and refunds any over-reservation. This means:

- The displayed remaining budget is always accurate.
- No request can cause overspend, even under high concurrency.
- Aborted streaming requests are still billed for the tokens actually delivered.

**No budget** — keys without a `budget_usd` field have unlimited spend (like the
admin key). Be careful with unlimited keys in production.

---

## Rate limits

Two dimensions, checked before every request:

| Limit | Flag | Description |
|---|---|---|
| `rpm` | `--rpm` | Maximum requests per minute |
| `tpm` | `--tpm` | Maximum tokens per minute (estimated from request; true-up after) |

When a rate limit is exceeded the gateway returns:

```http
HTTP/1.1 429 Too Many Requests
Retry-After: 12

{
  "error": {
    "type": "rate_limit_exceeded",
    "message": "Rate limit exceeded: 60 RPM",
    "code": "rate_limit_exceeded"
  }
}
```

The `Retry-After` header gives the number of seconds to wait.

---

## Model allowlists

A model allowlist restricts a key to specific model IDs. Requests for models not
on the list are rejected before any upstream call:

```http
HTTP/1.1 403 Forbidden

{
  "error": {
    "type": "model_not_allowed",
    "message": "Model 'gpt-4o' is not in the allowlist for this key"
  }
}
```

Allowlists use exact model ID matching. If you want to allow all variants of a
provider's models, create separate keys or use the admin key.

---

## Keys in config-as-code

Keys can also be declared in `oximy-gateway.json`:

```json
{
  "keys": [
    {
      "id": "key_alice",
      "name": "alice-dev",
      "budget_usd": 10.00,
      "model_allowlist": ["gpt-4o", "gpt-4o-mini"],
      "rpm": 120,
      "tpm": 500000
    },
    {
      "id": "key_ci",
      "name": "ci-pipeline",
      "budget_usd": 5.00,
      "model_allowlist": ["gpt-4o-mini"],
      "expires_at": "2026-12-31T00:00:00Z"
    }
  ]
}
```

Note: the **secret** is not stored in the config file. When `oximy-gateway config
apply` creates a new key from a config entry, it prints the secret once. If you
are applying the same config repeatedly (idempotent), existing keys are not
re-created and no secret is printed.

---

## Checking remaining budget

```bash
# In the request response:
# usage.cost tells you what this request spent

# Via the dashboard:
# Keys tab → click a key → see spent / budget

# Via the API (planned, P3):
# GET /v1/keys/{id}/budget
```

---

## Best practices

- **Never use the admin key in application code.** Create a scoped key for each
  application, service, or team member.
- **Set budgets on all application keys.** Even a large budget ($1000) prevents
  runaway costs from a bug or prompt-injection attack.
- **Set model allowlists for production keys.** This limits blast radius if a key
  is leaked — the attacker can only call models you have explicitly permitted.
- **Rotate keys periodically.** Revoke the old key, create a new one. The new
  key gets a fresh budget.
- **Use the `--expires` flag for temporary access.** Demo keys, hackathon keys,
  contractor access — set an expiry so you do not need to remember to revoke them.
