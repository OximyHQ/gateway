#!/usr/bin/env bash
# Oximy Gateway — automated review-checklist runner.
# Requires: the gateway running with the `everything` MCP server, and OXIMY_KEY set
# to the admin key. Usage:
#   export OXIMY_KEY=ogw_xxxxxxxx
#   ./run-checklist.sh           (or: bash run-checklist.sh)
set -u
BASE="${BASE:-http://127.0.0.1:8080}"
KEY="${OXIMY_KEY:-}"

pass=0; fail=0
ok() { printf "  \033[32mPASS\033[0m  %s\n" "$1"; pass=$((pass+1)); }
no() { printf "  \033[31mFAIL\033[0m  %s — %s\n" "$1" "$2"; fail=$((fail+1)); }
hdr() { printf "\n=== %s ===\n" "$1"; }

[ -n "$KEY" ] || { echo "ERROR: export OXIMY_KEY=ogw_... (your admin key) first"; exit 1; }
curl -sf "$BASE/health" >/dev/null || { echo "ERROR: gateway not reachable at $BASE — start it first"; exit 1; }

mint()      { curl -s "$BASE/v1/admin/keys" -H "Authorization: Bearer $KEY" -H 'Content-Type: application/json' -d "$1" | jq -r '.secret'; }
chat_code() { curl -s -o /dev/null -w '%{http_code}' "$BASE/v1/chat/completions" -H "Authorization: Bearer $1" -H 'Content-Type: application/json' -d "$2"; }
mcp()       { curl -s "$BASE/mcp" -H "Authorization: Bearer $1" -H 'Content-Type: application/json' -d "$2"; }

hdr "1. LLM call"
r=$(curl -s "$BASE/v1/chat/completions" -H "Authorization: Bearer $KEY" -H 'Content-Type: application/json' -d '{"model":"openai/gpt-4o-mini","messages":[{"role":"user","content":"Say PONG"}],"max_tokens":10}')
c=$(echo "$r" | jq -r '.choices[0].message.content // empty')
[ -n "$c" ] && ok "LLM call (\"$c\")" || no "LLM call" "$r"

hdr "2. Tool / function call"
r=$(curl -s "$BASE/v1/chat/completions" -H "Authorization: Bearer $KEY" -H 'Content-Type: application/json' -d '{"model":"openai/gpt-4o-mini","messages":[{"role":"user","content":"Weather in Tokyo?"}],"tools":[{"type":"function","function":{"name":"get_weather","parameters":{"type":"object","properties":{"city":{"type":"string"}}}}}],"tool_choice":"required","max_tokens":80}')
tc=$(echo "$r" | jq -r '.choices[0].message.tool_calls[0].function.name // empty')
[ "$tc" = "get_weather" ] && ok "Tool call (get_weather)" || no "Tool call" "$r"

hdr "3. Multimodal call"
r=$(curl -s "$BASE/v1/chat/completions" -H "Authorization: Bearer $KEY" -H 'Content-Type: application/json' -d '{"model":"openai/gpt-4o-mini","messages":[{"role":"user","content":[{"type":"text","text":"What animal? One word."},{"type":"image_url","image_url":{"url":"https://upload.wikimedia.org/wikipedia/commons/thumb/3/3a/Cat03.jpg/120px-Cat03.jpg"}}]}],"max_tokens":15}')
c=$(echo "$r" | jq -r '.choices[0].message.content // empty')
echo "$c" | grep -qi cat && ok "Multimodal (\"$c\")" || no "Multimodal" "$r"

hdr "4. Across several providers"
for M in openai/gpt-4o-mini anthropic/claude-3.5-haiku google/gemini-2.5-flash; do
  code=$(chat_code "$KEY" "{\"model\":\"$M\",\"messages\":[{\"role\":\"user\",\"content\":\"Say OK\"}],\"max_tokens\":10}")
  [ "$code" = "200" ] && ok "provider $M (200)" || no "provider $M" "http $code"
done

hdr "5. Native Gemini"
curl -s -D /tmp/_oxi_h -o /dev/null "$BASE/v1/chat/completions" -H "Authorization: Bearer $KEY" -H 'Content-Type: application/json' -d '{"model":"google/gemini-flash-latest","messages":[{"role":"user","content":"Capital of Japan? One word."}],"max_tokens":1000}'
sb=$(grep -i '^x-served-by' /tmp/_oxi_h | tr -d '\r' | sed 's/x-served-by: //')
echo "$sb" | grep -qi 'google/' && ok "Native Gemini (served-by $sb)" || no "Native Gemini" "served-by=$sb"

hdr "6. Virtual key + budget enforcement"
TINY=$(mint '{"name":"chk-tiny","budget_usd":0.00002}')
codes=""
for i in 1 2 3 4 5; do codes="$codes $(chat_code "$TINY" "{\"model\":\"openai/gpt-4o-mini\",\"messages\":[{\"role\":\"user\",\"content\":\"nonce $i $RANDOM\"}],\"max_tokens\":10}")"; done
echo "$codes" | grep -q 429 && ok "Budget enforced (codes:$codes)" || no "Budget enforced" "codes:$codes"

hdr "7. Model allowlist"
ALLOW=$(mint '{"name":"chk-allow","budget_usd":5,"models":["openai/gpt-4o-mini"]}')
a=$(chat_code "$ALLOW" '{"model":"openai/gpt-4o-mini","messages":[{"role":"user","content":"hi"}],"max_tokens":5}')
d=$(chat_code "$ALLOW" '{"model":"anthropic/claude-3.5-haiku","messages":[{"role":"user","content":"hi"}],"max_tokens":5}')
{ [ "$a" = "200" ] && [ "$d" = "403" ]; } && ok "Allowlist (allowed=$a disallowed=$d)" || no "Allowlist" "allowed=$a disallowed=$d"

hdr "8. Guardrail (secrets scanner)"
code=$(chat_code "$KEY" '{"model":"openai/gpt-4o-mini","messages":[{"role":"user","content":"store sk-abcdefghijklmnopqrstuvwxyz0123456789"}],"max_tokens":10}')
[ "$code" = "403" ] && ok "Secret blocked (403)" || no "Guardrail" "http $code"

hdr "9. MCP federation + call"
n=$(mcp "$KEY" '{"jsonrpc":"2.0","id":1,"method":"tools/list"}' | jq '.result.tools | length')
e=$(mcp "$KEY" '{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"everything__echo","arguments":{"message":"ok"}}}' | jq -r '.result.content[0].text // empty')
{ [ "${n:-0}" -ge 1 ] && [ -n "$e" ]; } && ok "MCP works ($n tools, echo=\"$e\")" || no "MCP federation" "tools=$n echo=$e — is the 'everything' server registered?"

hdr "10. Per-tool ACL — the 'NO DELETE' test"
RO=$(mint '{"name":"chk-acl","budget_usd":1,"tool_allowlist":["everything__echo","everything__get-sum"]}')
seen=$(mcp "$RO" '{"jsonrpc":"2.0","id":1,"method":"tools/list"}' | jq -c '.result.tools | map(.name) | sort')
blk=$(mcp "$RO" '{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"everything__get-env","arguments":{}}}' | jq -r '.error.code // empty')
{ [ "$seen" = '["everything__echo","everything__get-sum"]' ] && [ "$blk" = "-32000" ]; } \
  && ok "ACL enforced (key sees only its 2 tools; get-env blocked -32000)" \
  || no "Per-tool ACL" "sees=$seen blocked=$blk (want 2 tools + -32000)"

printf "\n========================================\n"
printf "  RESULT: \033[32m%d passed\033[0m, \033[31m%d failed\033[0m\n" "$pass" "$fail"
printf "========================================\n"
echo "(Documented gaps NOT tested here: add-provider via UI/API; real Linear MCP needs P2 OAuth.)"
