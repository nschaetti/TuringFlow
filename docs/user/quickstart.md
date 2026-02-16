# Quickstart

This quickstart gets you from clone to a running daemon and basic checks.

## Prerequisites

- Rust toolchain (`cargo`, `rustc`)
- Valid TLS materials (server cert/key + client CA) referenced in `config/turingflowd.yaml`

## Build

```bash
cargo build
```

## Check binaries

```bash
cargo run --bin turingflow -- --help
cargo run --bin turingflowd -- --help
```

## Start daemon

```bash
cargo run --bin turingflowd -- --config config/turingflowd.yaml --kingdoms-config config/kingdoms.yaml
```

## Validate health

Use mTLS and call:

`GET /tfpv1/health`

For a script-driven flow, see `scripts/tfpv1_curl_demo.sh` and `scripts/README.md`.

## Run key tests

```bash
cargo test --lib
cargo test --test tfpv1_integration
cargo test --test turingflowd_http_integration
```
