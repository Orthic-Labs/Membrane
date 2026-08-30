# Migrations

## Installation identity

Treat schema or data-root changes as a transaction. Verify source and target
installation IDs, schema version, data-root digest, and a fresh backup before
starting. Use the repository migration command for the installed release; record
the migration receipt and row/event counts.

## Schema migration

Repair boundary: do not hand-edit schema tables or delete rows to fix
`incompatible_schema`, `wrong_installation`, `unexpected_data_root`, or a stale
migration. Stop on any failed phase and use the recorded rollback path; escalate
if rollback is incomplete.
