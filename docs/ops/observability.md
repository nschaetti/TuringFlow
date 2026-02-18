# Observability

## Daemon metrics (`/tfpv1/health`)

- `messages_in`
- `messages_forwarded`
- `messages_failed`
- `dedupe_hits`

## Logs

Daemon logs include:

- startup/listen events
- message receive/forward/failure events
- cleanup and storage warnings

Output modes:

- file: NDJSON (`1 line = 1 JSON event`)
- console: fixed-width table with aligned columns

Rotation and retention:

- base file rotates when `logging.rotation.max_bytes` is reached
- rotated files are optionally compressed to `.gz`
- keep policy uses `logging.rotation.max_files`

Structured log fields:

- `timestamp` (UTC ISO8601 with milliseconds)
- `level` (`TRACE|DEBUG|INFO|WARN|ERROR|FATAL`)
- `service`, `agent_id`, `node_id`
- `trace_id`, `span_id`, `parent_span_id`
- `event_type` (`SYSTEM`, `NETWORK`, `LLM_CALL`, `TOOL_CALL`, `MEMORY_READ`, `MEMORY_WRITE`, `PERFORMANCE`, `ERROR`, `SECURITY`)
- `message`, `context`, `duration_ms`

Secrets in context are redacted before write (`api_key`, `token`, `authorization`, etc.).

## Runtime level overrides

Protected debug endpoints (mTLS identity required):

- `GET /tfpv1/debug/logging-levels`: current global + per-agent + per-trace overrides
- `POST /tfpv1/debug/logging-levels`: set/clear overrides at runtime

Payload example:

```json
{
  "scope": "trace",
  "key": "4bf92f3577b34da6a3ce929d0e0e4736",
  "level": "debug",
  "ttl_ms": 600000
}
```

## Kernel audit logs

Table: `syscall_audit_log`

Important fields:

- `ts_ms`, `trace_id`, `kingdom_id`
- `principal_id`, `agent_ref`, `tool_id`
- `syscall`, `resource_json`
- `decision`, `rule_id`, `error_code`

Use this table to explain and prove allow/deny policy decisions.
