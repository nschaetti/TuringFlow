# Contributing

## Development workflow

1. Implement change.
2. Add/update tests.
3. Run checks locally.
4. Open PR with rationale and risk notes.

## Local checks

```bash
cargo fmt
cargo check
cargo test --lib
cargo test --test tfpv1_integration
cargo test --test turingflowd_http_integration
bash scripts/check_tooling_no_direct_host_access.sh
```

## Coding expectations

- preserve mTLS and identity guarantees
- preserve deny-by-default semantics
- no direct host access in tools runtime perimeter
- keep error contracts stable (`version`, `error.*`)
