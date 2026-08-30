# Maintenance planning

MBR-508 defines pure, low-priority planning for consolidation, contradiction detection, index maintenance, & proposal creation. `plan_maintenance` returns either one bounded `MaintenanceOperation` or a content-free `MaintenanceReceipt`; host code executes operations outside request/foreground handling.

Every request carries a caller-supplied authority receipt checked through a trusted `MaintenanceAuthorityVerifier`; wire data cannot assert `verified`. Scheduler identity must equal receipt subject, but never receipt issuer. Scope must be exactly `maintenance` & receipt must allow requested maintenance kind. Empty/bad identity, failed verification, cancellation, budget outside 1–10,000 units, or deadline outside next 15 minutes rejects planning.

No function here opens storage, changes an index, creates a proposal, creates a thread, or schedules a timer. Returned operations bind scheduler & authority receipt IDs; hosts persist receipts through established receipt plane & enforce budget/deadline/cancellation during execution.

## Store-side execution

`cortex-store::maintenance_exec` executes a planned, bounded operation against the Cortex SQLite store. It never certifies authority: a missing `authorityReceiptId` is refused before any transaction opens, and the module has no verification logic of its own — only the upstream planner's `plan_maintenance` call ever grants that field.

`MemDb::execute_bounded_maintenance` runs every offered unit inside one `IMMEDIATE` transaction and commits only when the whole job completes within budget & deadline without cancellation. Any cancellation, deadline expiry, budget exhaustion, or unit failure drops the transaction uncommitted instead, so SQLite rolls it back — including across a hard crash, since an uncommitted transaction's frames are never checkpointed into the main database file. A bounded maintenance job is therefore all-or-nothing: the store is never observed holding a partially-applied job, and a job that does not fit its bound can simply be retried as a smaller request. Every invocation that passes the authority check returns a content-free `MaintenanceExecReceipt` recording the outcome for audit.
