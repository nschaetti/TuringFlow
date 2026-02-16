# Database Migrations

## Mechanism

- Migrations are applied at daemon startup.
- Applied versions are recorded in `schema_migrations`.
- Migration execution is idempotent.

## Current versions

- `0001_tfpv1_core`
- `0002_kernel_policy_audit`

## Safety notes

- Back up SQLite DB before deploying new migration sets.
- Validate startup in staging before production rollouts.
- Keep migrations append-only; do not edit already shipped migration files.
