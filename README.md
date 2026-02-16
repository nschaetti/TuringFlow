# TuringFlow

[![Rust CI](https://github.com/nschaetti/TuringFlow/actions/workflows/rust.yml/badge.svg)](https://github.com/nschaetti/TuringFlow/actions/workflows/rust.yml)
[![codecov](https://codecov.io/gh/nschaetti/TuringFlow/branch/main/graph/badge.svg)](https://codecov.io/gh/nschaetti/TuringFlow)

<p align="center">
    <picture>
        <source media="(prefers-color-scheme: light)" srcset="https://raw.githubusercontent.com/nschaetti/TuringFlow/refs/heads/main/images/turingflow_banner.png">
        <img src="https://raw.githubusercontent.com/nschaetti/TuringFlow/refs/heads/main/images/turingflow_banner.png" alt="TuringFlow" width="500">
    </picture>
</p>

TuringFlow is an agent transport + runtime foundation with:

- a secure `TFPv1` daemon (`turingflowd`) over mTLS,
- registry/routing/ack/dedupe persistence in SQLite,
- a kernel-style access control model for agent operations,
- CLI tooling for model interactions.

## Current scope

- `turingflowd` API endpoints: health, register, heartbeat, resolve, send, ack.
- Config-driven runtime:
  - `config/turingflowd.yaml`
  - `config/kingdoms.yaml`
  - `config/policies.yaml`
- Kernel syscalls and policy engine:
  - `fs.list/read/write`
  - `proc.exec`
  - `net.http`
  - deny-by-default + audit log in SQLite.

## Quick start

Build:

```bash
cargo build
```

Show CLI help:

```bash
cargo run --bin turingflow -- --help
```

Show daemon help:

```bash
cargo run --bin turingflowd -- --help
```

Run daemon (requires valid cert files in config):

```bash
cargo run --bin turingflowd -- --config config/turingflowd.yaml --kingdoms-config config/kingdoms.yaml
```

## Documentation

- User docs: `docs/user/quickstart.md`
- Developer docs: `docs/dev/architecture.md`
- Operations docs: `docs/ops/runbook.md`
- Full index: `docs/README.md`

## Testing

Run all key test suites:

```bash
cargo test --lib
cargo test --test tfpv1_integration
cargo test --test turingflowd_http_integration
```

Tooling security lint (no direct host access in tools perimeter):

```bash
bash scripts/check_tooling_no_direct_host_access.sh
```
