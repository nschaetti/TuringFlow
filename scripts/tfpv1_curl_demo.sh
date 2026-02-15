#!/usr/bin/env bash
set -euo pipefail

# Required env vars:
#   TFP_BASE_URL          ex: https://127.0.0.1:8443
#   TFP_CA_CERT           ex: certs/ca.crt
#   TFP_NODE_A_CERT       ex: certs/node-a.crt
#   TFP_NODE_A_KEY        ex: certs/node-a.key
#   TFP_NODE_B_CERT       ex: certs/node-b.crt
#   TFP_NODE_B_KEY        ex: certs/node-b.key
#   NODE_A_DELIVER_URL    ex: https://127.0.0.1:9443
#   NODE_B_DELIVER_URL    ex: https://127.0.0.1:9444

: "${TFP_BASE_URL:?missing}"
: "${TFP_CA_CERT:?missing}"
: "${TFP_NODE_A_CERT:?missing}"
: "${TFP_NODE_A_KEY:?missing}"
: "${TFP_NODE_B_CERT:?missing}"
: "${TFP_NODE_B_KEY:?missing}"
: "${NODE_A_DELIVER_URL:?missing}"
: "${NODE_B_DELIVER_URL:?missing}"

NOW_UTC="$(date -u +%Y-%m-%dT%H:%M:%SZ)"

echo "== register agent A =="
curl --silent --show-error --fail \
  --cacert "$TFP_CA_CERT" \
  --cert "$TFP_NODE_A_CERT" \
  --key "$TFP_NODE_A_KEY" \
  -H "Content-Type: application/json" \
  -d "{
    \"version\": \"TFPv1\",
    \"kingdom_id\": \"kingdom-main\",
    \"node\": {
      \"node_id\": \"node-a\",
      \"hostname\": \"node-a.local\",
      \"deliver_url\": \"$NODE_A_DELIVER_URL\"
    },
    \"agents\": [
      { \"agent_ref\": \"planner@node-a.local\", \"agent_id\": \"ag_01A\" }
    ],
    \"lease_ttl_ms\": 45000
  }" \
  "$TFP_BASE_URL/tfpv1/agents/register"
echo

echo "== register agent B =="
curl --silent --show-error --fail \
  --cacert "$TFP_CA_CERT" \
  --cert "$TFP_NODE_B_CERT" \
  --key "$TFP_NODE_B_KEY" \
  -H "Content-Type: application/json" \
  -d "{
    \"version\": \"TFPv1\",
    \"kingdom_id\": \"kingdom-main\",
    \"node\": {
      \"node_id\": \"node-b\",
      \"hostname\": \"node-b.local\",
      \"deliver_url\": \"$NODE_B_DELIVER_URL\"
    },
    \"agents\": [
      { \"agent_ref\": \"executor@node-b.local\", \"agent_id\": \"ag_01B\" }
    ],
    \"lease_ttl_ms\": 45000
  }" \
  "$TFP_BASE_URL/tfpv1/agents/register"
echo

echo "== resolve executor@node-b.local =="
curl --silent --show-error --fail \
  --cacert "$TFP_CA_CERT" \
  --cert "$TFP_NODE_A_CERT" \
  --key "$TFP_NODE_A_KEY" \
  "$TFP_BASE_URL/tfpv1/agents/resolve/executor@node-b.local?kingdom_id=kingdom-main"
echo

echo "== send planner -> executor =="
curl --silent --show-error --fail \
  --cacert "$TFP_CA_CERT" \
  --cert "$TFP_NODE_A_CERT" \
  --key "$TFP_NODE_A_KEY" \
  -H "Content-Type: application/json" \
  -d "{
    \"version\": \"TFPv1\",
    \"kingdom_id\": \"kingdom-main\",
    \"message\": {
      \"version\": \"TFPv1\",
      \"message_id\": \"msg_demo_01\",
      \"trace_id\": \"trc_demo_01\",
      \"timestamp\": \"$NOW_UTC\",
      \"from_ref\": \"planner@node-a.local\",
      \"to_ref\": \"executor@node-b.local\",
      \"kind\": \"request\",
      \"ttl_ms\": 10000,
      \"requires_ack\": true,
      \"routing\": { \"hops_max\": 8, \"path\": [] },
      \"payload\": {
        \"content_type\": \"application/json\",
        \"body\": { \"cmd\": \"run\", \"input\": \"hello\" }
      },
      \"meta\": { \"priority\": \"normal\", \"tags\": [\"demo\"] }
    }
  }" \
  "$TFP_BASE_URL/tfpv1/messages/send"
echo

echo "== ack async example =="
curl --silent --show-error --fail \
  --cacert "$TFP_CA_CERT" \
  --cert "$TFP_NODE_B_CERT" \
  --key "$TFP_NODE_B_KEY" \
  -H "Content-Type: application/json" \
  -d "{
    \"version\": \"TFPv1\",
    \"delivery_id\": \"dlv_demo_01\",
    \"message_id\": \"msg_demo_01\",
    \"from_ref\": \"executor@node-b.local\",
    \"status\": \"processed\",
    \"timestamp\": \"$NOW_UTC\",
    \"result\": { \"ok\": true }
  }" \
  "$TFP_BASE_URL/tfpv1/messages/ack"
echo
