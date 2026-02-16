# Troubleshooting

## Daemon fails at startup

Check:

- YAML syntax in `config/turingflowd.yaml` and `config/kingdoms.yaml`
- TLS file paths exist and are readable
- config `version` fields are `1`

## mTLS request rejected

Check:

- client cert is signed by configured `client_ca_cert`
- client cert identity matches `node_id` in payload

## `kingdom_not_allowed`

Your `kingdom_id` is not enabled in `config/kingdoms.yaml`.

## `payload_too_large` or TTL errors

The request exceeds global or per-kingdom limits.

## `EACCES` on local agent operation

The operation was blocked by kernel policy:

- path outside allowed prefix
- command/method/host not allowed
- operation not allowed for agent/tool principal

## Debug commands

```bash
cargo test --lib
cargo test --test tfpv1_integration
cargo test --test turingflowd_http_integration
bash scripts/check_tooling_no_direct_host_access.sh
```
