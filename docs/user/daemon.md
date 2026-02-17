# Daemon Usage (`turingflowd`)

`turingflowd` is the TFPv1 endpoint implementing secure registration, routing, and acknowledgements.

## Start

```bash
cargo run --bin turingflowd -- --config config/turingflowd.yaml --kingdoms-config config/kingdoms.yaml
```

## Options

- `--config` path to daemon config (`config/turingflowd.yaml` by default)
- `--kingdoms-config` path to kingdoms quotas/allowlist (`config/kingdoms.yaml` by default)
- `--channels-config` path to user channels config (`config/channels.yaml` by default)

## Main API endpoints

- `GET /tfpv1/health`
- `POST /tfpv1/agents/register`
- `POST /tfpv1/agents/heartbeat`
- `GET /tfpv1/agents/resolve/{agent_ref}?kingdom_id=...`
- `POST /tfpv1/messages/send`
- `POST /tfpv1/messages/ack`
- `GET /tfpv1/debug/user-queues?limit=50&include_acked=false&include_delivered=false`

## Security behavior

- Mutual TLS is required.
- Requesting node identity must match client certificate identity.
- Kingdom allowlist/quotas are enforced.
- Replay/duplicate checks are enforced for messages.

## Local agent operations

`send` supports local kernel-guarded operations when payload type is:

`application/vnd.turingflow.agent-op+json`

Supported operation keys:

- `fs.read`
- `fs.list`
- `fs.write`
- `proc.exec`
- `net.http`
- `user.ingest`
- `user.recv`
- `user.send`
- `user.inbox`
- `user.route.resolve`

Unauthorized operations return structured errors (for example `EACCES`).

## Matrix worker

When `channels.matrix.enabled` is true and its token environment variable is available,
`turingflowd` starts a background worker that:

- ingests Matrix room messages into `user_inbound`
- delivers queued `user_outbound` messages to Matrix
- updates outbound status (`delivered` or `failed`)
