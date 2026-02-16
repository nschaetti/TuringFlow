# Storage and Persistence

Backend: SQLite

## Runtime PRAGMAs

- `journal_mode = WAL`
- `synchronous = NORMAL`
- `busy_timeout = 5000`
- `foreign_keys = ON`

## Migrations

- `0001_tfpv1_core.sql`
  - leases
  - agents
  - dedupe
  - acks
- `0002_kernel_policy_audit.sql`
  - policy_versions
  - policy_rules
  - principal_bindings
  - syscall_audit_log

## Key behavior

- migrations are idempotent and recorded in `schema_migrations`
- startup initializes DB and applies pending migrations
- audit retention uses periodic purge in `SqliteAuditSink`
