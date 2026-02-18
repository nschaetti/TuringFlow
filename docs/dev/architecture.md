# Architecture

## High-level modules

- `src/bin/turingflowd.rs`: daemon entrypoint, API routes, mTLS handling.
- `src/tfpv1/*`: protocol types, registry, router, dedupe, config loading.
- `src/tfpv1/storage/*`: SQLite initialization and persistence modules.
- `src/kernel/*`: policy engine, syscall kernel, providers, audit sink.
- `src/observability/*`: structured logging, tracing context, runtime level controls.
- `src/commands/*`: CLI command handlers and tooling runtime wrappers.

## Transport planes

- Inter-agent plane: `TFPv1` (`turingflowd`) for node/agent messaging.
- User communication plane: kernel `user.*` syscalls for messages coming from or going to the human user.
- Channel connectors (Matrix, Email, Webhook, etc.) are adapters to the user plane, not TFPv1 peers.
- A Matrix connector worker currently runs in `turingflowd` and syncs `user_inbound` / `user_outbound` queues.

## Request flow (`send`)

1. Validate request and kingdom quotas.
2. Replay and dedupe checks.
3. Verify source/destination agents.
4. If local agent-op payload, execute via kernel syscalls.
5. Else route message to destination deliver URL.

## Data flow

- Config YAML -> validated structs.
- State persistence -> SQLite tables/migrations.
- Policy decisions -> audit records in `syscall_audit_log`.
