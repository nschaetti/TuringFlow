# TFPv1 API

Base path: `/tfpv1`

## Endpoints

- `GET /health`
- `POST /agents/register`
- `POST /agents/heartbeat`
- `GET /agents/resolve/{agent_ref}?kingdom_id=...`
- `POST /messages/send`
- `POST /messages/ack`

## `send` local agent operation

If payload `content_type` is `application/vnd.turingflow.agent-op+json`, daemon may execute local operation through kernel.

Example payload body:

```json
{
  "op": "fs.read",
  "path": "/workspace/project/file.txt"
}
```

Supported ops:

- `fs.read`, `fs.list`, `fs.write`
- `proc.exec`
- `net.http`

## Error contracts

All errors include:

- `version`
- `error.code`
- `error.message`
- `error.retryable`

Common daemon errors:

- `invalid_payload`
- `identity_mismatch`
- `kingdom_not_allowed`
- `payload_too_large`
- `duplicate_message`

Kernel-originated errors in local ops use code values like:

- `EACCES`, `ENOENT`, `EINVAL`, `ETIMEOUT`, `ERATELIMIT`, `EINTERNAL`
