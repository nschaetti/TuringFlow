# Testing Guide

## Unit tests

Run library tests:

```bash
cargo test --lib
```

Coverage includes:

- policy matching and priority
- filesystem normalization and escape protections
- process/network allowlist controls
- storage components (registry, dedupe, ack)

## Integration tests

Core integration suite:

```bash
cargo test --test tfpv1_integration
```

Daemon HTTP/mTLS integration suite:

```bash
cargo test --test turingflowd_http_integration
```

## Coverage

Local coverage generation (same tool as CI):

```bash
cargo install cargo-llvm-cov
cargo llvm-cov --workspace --lcov --output-path lcov.info --fail-under-lines 30
```

In GitHub Actions, coverage is:

- generated with a minimum line threshold (`COVERAGE_MIN_LINES`, currently `30`)
- uploaded to Codecov (OIDC)
- uploaded as `rust-coverage-lcov` artifact (`lcov.info`)

## Security lint for tools runtime

```bash
bash scripts/check_tooling_no_direct_host_access.sh
```

This verifies no direct host access APIs are used in the tools runtime perimeter.
