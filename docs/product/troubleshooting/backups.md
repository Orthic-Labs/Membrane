# Backups

## Data-root boundary

Before any repair, copy the active data root and its manifest/receipt to a new,
operator-controlled location and record a checksum. Keep the original untouched
until verification succeeds. Do not call a cache, stale snapshot, or diagnostic
bundle a backup: bundles are content-free evidence, not restorable state.

Repair boundary: never delete, compact, reindex, or overwrite an active root to
clear a Hub alert. Restore only an identified last-known-good backup, then rerun
the same read-only checks and retain both receipts.
