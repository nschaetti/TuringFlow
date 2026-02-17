# Configuration

TuringFlow uses YAML config files.

## `config/turingflowd.yaml`

Controls daemon behavior.

Key sections:

- `version`: config schema version (must be `1`)
- `server`: `listen`, `node_id`
- `tls`: server cert/key, client CA, optional upstream TLS certs
- `security`: replay window
- `routing`: retry delays
- `storage`: SQLite backend/path
- `limits`: max payload size and max TTL
- `logging`: format and level

Validation is strict at boot. Invalid or missing values fail startup.

## `config/kingdoms.yaml`

Defines allowed kingdoms and quotas.

Fields:

- `id`
- `enabled`
- `quotas.max_agents_per_node`
- `quotas.max_lease_ttl_ms`
- `quotas.max_message_ttl_ms`
- `quotas.max_payload_bytes`

`register`, `heartbeat`, `resolve`, and `send` all check kingdom validity.

## `config/policies.yaml`

Defines kernel authorization policies.

Core model:

- `defaults.decision` is `deny`
- principal can be `agent:<agent_ref>` or `agent_tool:<agent_ref>:<tool_id>`
- rules match syscall + optional resource constraints

Example rule concepts:

- `path_prefix` for `fs.*`
- `command_allowlist` for `proc.exec`
- `host_allowlist` and `methods` for `net.http`

## `config/channels.yaml`

Defines user communication connectors and default routing.

Current scope:

- single-user mode (`user.mode: single`)
- phase-1 channels: `matrix`, `email`, `webhook`
- planned channels (disabled by default): `slack`, `discord`

Connectors should map channel events into kernel `user.*` syscalls (`user.ingest`, `user.send`, `user.inbox`, `user.route.resolve`).
