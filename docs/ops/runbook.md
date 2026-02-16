# Operations Runbook

## Start daemon

```bash
cargo run --bin turingflowd -- --config config/turingflowd.yaml --kingdoms-config config/kingdoms.yaml
```

## Verify service

- Health endpoint: `GET /tfpv1/health`
- Logs: verify listener bind and request activity

## Common incidents

- startup fails: invalid config or missing TLS files
- request denied: identity mismatch, kingdom disabled, quota exceeded
- kernel deny: `EACCES` for unauthorized local operations

## Recovery

- fix configuration and restart
- inspect SQLite state and audit logs
- re-run integration tests if behavior regressions are suspected
