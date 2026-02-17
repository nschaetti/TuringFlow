# Security Model

TuringFlow applies layered controls.

## Transport and identity

- mTLS required for daemon API.
- Node identity is extracted from the client certificate.
- Identity mismatch is rejected.

## Kingdom boundaries

- Only configured kingdoms are accepted.
- Per-kingdom quotas are enforced.

## Message integrity

- RFC3339 timestamps and replay window checks.
- Duplicate message rejection (`message_id` dedupe).

## Kernel access control

- Syscalls are deny-by-default.
- Policy precedence: `agent_tool` then `agent`.
- File, process, and network access can be rejected with OS-like codes (`EACCES`, `ENOENT`, etc.).
- User-plane communication (`user.*`) is also policy-gated through kernel syscalls.

## Auditability

- Every policy decision is written to `syscall_audit_log`.
- Audit records include trace, principal, syscall, decision, and error code.
