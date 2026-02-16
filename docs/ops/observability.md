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

## Kernel audit logs

Table: `syscall_audit_log`

Important fields:

- `ts_ms`, `trace_id`, `kingdom_id`
- `principal_id`, `agent_ref`, `tool_id`
- `syscall`, `resource_json`
- `decision`, `rule_id`, `error_code`

Use this table to explain and prove allow/deny policy decisions.
