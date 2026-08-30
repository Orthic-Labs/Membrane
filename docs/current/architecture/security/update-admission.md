# Dual-signature update admission (MBR-911)

An update becomes eligible only when trusted adapters independently verify its
Tauri updater signature & platform trust. macOS requires platform signature plus
notarization. Windows requires Public Trust signature plus RFC3161 timestamp.

Wire evidence carries artifact identity, signatures, key ID, platform, & receipt
ID; it carries no caller-asserted validity booleans. Both evidence records must
bind exact lowercase SHA-256 artifact identity before cryptographic verification.

`membrane-updater::verify` is pure admission. It has no filesystem, process,
download, migration, or activation API. Failure returns content-free
`BlockedUpdate` evidence with stable codes, versions, artifact hash, platform,
evidence IDs, & `repair/update-signatures`; current version therefore remains
active by construction.

Successful `VerifiedUpdate` permits an existing transaction to continue; it does
not activate anything. Node source checks are static & perform no update.
