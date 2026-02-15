# TFPv1 Scripts

- `agent_sim.py`: minimal HTTPS receiver exposing `POST /tfpv1/deliver`.
- `tfpv1_curl_demo.sh`: mTLS curl flow for register/resolve/send/ack.

## Start 2 simulated agents

```bash
python scripts/agent_sim.py --node node-a --port 9443 --tls-cert certs/node-a.crt --tls-key certs/node-a.key
python scripts/agent_sim.py --node node-b --port 9444 --tls-cert certs/node-b.crt --tls-key certs/node-b.key
```

## Run curl flow

```bash
export TFP_BASE_URL="https://127.0.0.1:8443"
export TFP_CA_CERT="certs/ca.crt"
export TFP_NODE_A_CERT="certs/node-a.crt"
export TFP_NODE_A_KEY="certs/node-a.key"
export TFP_NODE_B_CERT="certs/node-b.crt"
export TFP_NODE_B_KEY="certs/node-b.key"
export NODE_A_DELIVER_URL="https://127.0.0.1:9443"
export NODE_B_DELIVER_URL="https://127.0.0.1:9444"

bash scripts/tfpv1_curl_demo.sh
```
