# Diagnostics

## Read-only diagnostics

Use read-only checks first. Capture the Hub snapshot timestamp, resource, reason,
evidence, resolver, and trace ID (if present). Run `membrane cli doctor --bundle
./membrane-diagnostic.json` for a content-free bundle; do not paste prompts, rows,
tokens, local paths, or repair text into tickets. A missing reader stays
`unavailable`; never infer health from process existence.

Repair boundary: diagnostics may inspect and export; they must not rewrite the DB,
rotate credentials, restart services, or change installation roots. Escalate with
the exact reason and bundle when the safe action does not restore evidence.
