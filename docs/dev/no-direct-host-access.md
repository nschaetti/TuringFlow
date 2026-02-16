# No Direct Host Access Policy

Tools/plugins in the runtime perimeter must not directly access host resources.

## Required pattern

- Use kernel wrappers/providers (`ToolRuntime`, `Kernel`) for FS/process/network access.

## Forbidden direct APIs in tools perimeter

- `std::fs`
- `std::process::Command`
- `tokio::fs`
- `tokio::process`
- `reqwest::blocking::Client`
- `image::open(...)`

## Enforcement

CI job runs:

```bash
bash scripts/check_tooling_no_direct_host_access.sh
```

If direct host API usage is detected in `src/commands` (except `runtime.rs`) or `src/rchain/tools.rs`, CI fails.
