# Maintenance planning

MBR-508 defines pure, low-priority planning for consolidation, contradiction detection, index maintenance, & proposal creation. `plan_maintenance` returns either one bounded `MaintenanceOperation` or a content-free `MaintenanceReceipt`; host code executes operations outside request/foreground handling.

Every request carries a caller-supplied authority receipt checked through a trusted `MaintenanceAuthorityVerifier`; wire data cannot assert `verified`. Scheduler identity must equal receipt subject, but never receipt issuer. Scope must be exactly `maintenance` & receipt must allow requested maintenance kind. Empty/bad identity, failed verification, cancellation, budget outside 1–10,000 units, or deadline outside next 15 minutes rejects planning.

No function here opens storage, changes an index, creates a proposal, creates a thread, or schedules a timer. Returned operations bind scheduler & authority receipt IDs; hosts persist receipts through established receipt plane & enforce budget/deadline/cancellation during execution.
