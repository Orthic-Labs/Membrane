# Update rollback

For a failed update, stop new work, preserve the failed receipt, and select the
receipt-bound last accepted release. The update transaction restores the prior
active directory and rolls back partial or complete schema changes; verify health
and Hub identity after rollback.

Repair boundary: never swap binaries manually, remove `.rollback`, or rerun a
failed migration against the live root. If rollback reports filesystem or schema
errors, keep the service stopped and escalate with both receipts and the diagnostic
bundle.
