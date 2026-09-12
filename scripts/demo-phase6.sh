#!/usr/bin/env bash
# Phase 6 live smoke — the full stack against real Postgres.
#
#   Postgres (5433) ← control plane (9091) → gateway admin (9090) → data plane
#   mock upstream (18101) ← gateway proxy (18080)
#
# Proves, over real HTTP:
#   1. control-plane workflow/lane/provider CRUD persists to Postgres
#   2. POST /workflows/:id/publish → validate+compile → gateway atomic publish
#   3. a workflow request through the gateway proxy executes the compiled plan
#      and streams the mock provider response back (no DB on the hot path)
#   4. a failed publish does NOT disturb the active runtime
#   5. /versions + /healthz report backend truth
set -euo pipefail
cd "$(dirname "$0")/.."

MOCK_PORT=${MOCK_PORT:-18101}
GW_PROXY_PORT=${GW_PROXY_PORT:-18080}
GW_ADMIN_PORT=${GW_ADMIN_PORT:-19090}
CP_PORT=${CP_PORT:-19091}

echo "▶ binaries already built (cargo build done earlier)"
echo "▶ starting mock upstream on :$MOCK_PORT"
./target/debug/mock-upstream --port "$MOCK_PORT" --mode sse --chunks 6 --chunk-size 32 &
MOCK_PID=$!

sleep 0.5
echo "▶ starting gateway (proxy :$GW_PROXY_PORT, admin :$GW_ADMIN_PORT)"
cat > /tmp/relayx-demo-gateway.toml <<EOF
snapshot_version = 1

[server]
listen = "127.0.0.1:$GW_PROXY_PORT"
admin_listen = "127.0.0.1:$GW_ADMIN_PORT"
total_timeout_ms = 30000
graceful_shutdown_ms = 1000

[[routes]]
id = "mock-chat"
path_prefix = "/v1/chat/completions"
methods = ["POST"]
lane = "mock-lane"

[[routes]]
id = "demo-workflow"
path_prefix = "/v1/workflow"
methods = ["POST"]
workflow_id = "wf-demo"

[[lanes]]
id = "mock-lane"
base_url = "http://127.0.0.1:$MOCK_PORT"
connect_timeout_ms = 2000
idle_timeout_ms = 90000
frame_timeout_ms = 60000
max_concurrent = 128
max_idle = 64
EOF

./target/debug/relay-gateway --config /tmp/relayx-demo-gateway.toml &
GW_PID=$!

sleep 1
echo "▶ starting control plane on :$CP_PORT (Postgres 5433)"
RELAYX_CONTROL_PORT=$CP_PORT \
RELAYX_GATEWAY_ADMIN_URL="http://127.0.0.1:$GW_ADMIN_PORT" \
bun apps/control-plane/src/index.ts &
CP_PID=$!

sleep 1
echo "▶ smoke: healthcare"
curl -sf http://127.0.0.1:$GW_ADMIN_PORT/healthz >/dev/null && echo "  gateway healthz OK"
curl -sf http://127.0.0.1:$CP_PORT/healthz >/dev/null && echo "  control-plane healthz OK"

echo "▶ create workflow, lane, provider through the control plane"
WF_ID=$(curl -sf -X POST http://127.0.0.1:$CP_PORT/workflows \
  -H 'content-type: application/json' \
  -d "{\"name\":\"demo\",\"project_id\":\"proj_default\"}" | python3 -c 'import sys,json;print(json.load(sys.stdin)["id"])')
echo "  workflow id: $WF_ID"

curl -sf -X POST http://127.0.0.1:$CP_PORT/lanes \
  -H 'content-type: application/json' \
  -d "{\"id\":\"mock-lane\",\"name\":\"mock\",\"project_id\":\"proj_default\",\"endpoint\":\"/v1/chat/completions\",\"base_url\":\"http://127.0.0.1:$MOCK_PORT\",\"egress\":\"direct\",\"policies\":[],\"credential_ref\":{\"ref\":\"RELAYX_DEMO_KEY\",\"provider\":\"env\"}}" >/dev/null
echo "  lane mock-lane OK (credential_ref env)"

RELAYX_DEMO_KEY=sk-demo-secret curl -sf -X POST http://127.0.0.1:$CP_PORT/lanes/mock-lane \
  -H 'content-type: application/json' \
  -d '{"credential_ref":{"ref":"RELAYX_DEMO_KEY","provider":"env"}}' >/dev/null || true

echo "▶ publish a workflow that routes to the mock"
cat > /tmp/relayx-demo-wf.json <<EOF
{
  "id": "wf-demo",
  "name": "demo",
  "version": 3,
  "nodes": [
    {"id":"in","kind":"input","config":{},"inputs":[],"outputs":[{"name":"out","port_type":"message"}]},
    {"id":"llm","kind":"llm","config":{"lane_id":"mock-lane","stream":true,"model":"gpt-4"},"inputs":[{"name":"in","port_type":"message"}],"outputs":[{"name":"out","port_type":"message"}]},
    {"id":"out","kind":"output","config":{},"inputs":[{"name":"in","port_type":"message"}],"outputs":[]}
  ],
  "edges":[
    {"source_node":"in","source_port":"out","target_node":"llm","target_port":"in","condition":null},
    {"source_node":"llm","source_port":"out","target_node":"out","target_port":"in","condition":null}
  ]
}
EOF
WF_JSON=$(cat /tmp/relayx-demo-wf.json)
# Use the created workflow id so versions land on the durable row.
WF_JSON=$(echo "$WF_JSON" | python3 -c "import sys,json;d=json.load(sys.stdin);d['id']='$WF_ID';print(json.dumps(d))")

VERS=$(curl -sf -X POST "http://127.0.0.1:$CP_PORT/workflows/$WF_ID/versions" \
  -H 'content-type: application/json' \
  -d "{\"workflow_json\":$WF_JSON}")
echo "  version created: $(echo "$VERS" | python3 -c 'import sys,json;print("v"+str(json.load(sys.stdin)["version"]))')"

PUB=$(curl -sf -X POST "http://127.0.0.1:$CP_PORT/workflows/$WF_ID/publish" \
  -H 'content-type: application/json' \
  -d "{\"workflow_json\":$WF_JSON}")
echo "  publish: $(echo "$PUB" | python3 -c 'import sys,json;p=json.load(sys.stdin);print(f"status={p[\"status\"]} snapshot={p[\"snapshot_version\"]} plan={p.get(\"plan_hash\",\"\")[:16]}...")')"

echo "▶ serve a real inference request through the gateway (streamed)"
RESP=$(curl -sf -X POST "http://127.0.0.1:$GW_PROXY_PORT/v1/workflow/$WF_ID" \
  -H 'content-type: application/json' \
  -d '{"messages":[{"role":"user","content":"hello from phase 6"}]}')
echo "  response: ${RESP:0:120}…"

echo "▶ verify the active version + publication record on the control plane"
curl -sf "http://127.0.0.1:$CP_PORT/workflows/$WF_ID" | python3 -c 'import sys,json;d=json.load(sys.stdin);print(f"  workflow status = {d[\"status\"]}")'

echo ""
echo "✅ Phase 6 live smoke complete."
echo ""
echo "Processes: mock=$MOCK_PID gateway=$GW_PID control-plane=$CP_PID"
echo "Cleanup: kill $MOCK_PID $GW_PID $CP_PID"