# Kernel and Syscalls

The kernel layer provides controlled host access for agent operations.

## Core components

- `ExecutionContext`: trace/kingdom/agent/tool identity context.
- `PolicyEngine`: evaluates `allow/deny` for syscall + resource.
- `Kernel`: façade calling policy then provider.
- Providers:
  - `HostFsProvider`
  - `HostProcessProvider`
  - `HostNetworkProvider`

## Policy semantics

- deny-by-default
- principal priority: `agent_tool` > `agent`
- per-principal rule ordering by priority
- resource matching supports:
  - `path_prefix`
  - `command_allowlist`
  - `host_allowlist`
  - `methods`

## Filesystem safety

- canonicalization before access
- parent canonicalization for writes
- reject traversal components (`..`, `.`)
- reject symlink escapes outside root

## Process safety

- allowlist of binaries
- optional allowlist of args per binary
- no shell binaries allowed (`sh`, `bash`, ...)
- no path binaries in command (`/bin/...` rejected)

## Network safety

- allowlist hosts
- allowlist methods
- enforce timeout max

## Error mapping

Kernel uses OS-like codes:

- `EACCES`, `ENOENT`, `EINVAL`, `ETIMEOUT`, `ERATELIMIT`, `EINTERNAL`

In daemon API, these are returned in structured error payloads.
