# Compression receipts

`CompressionReceiptV1` records every bounded transform: original content hash,
transform/version, kept & protected spans, dropped spans with reasons, resolver,
risk, and token delta. A lossy receipt must recover its immutable original through
`resolver`; unavailable or hash-mismatched sources return `ImmutableSourceError`.

Schema version `1` & lowercase `sha256:<64 hex>` hashes match Rust validation.
Canonical serde bytes sort every key without whitespace. Receipt & nested span
objects reject unknown fields; contract strings are nonempty & span offsets are
bounded to Rust `u32`. Recovery errors structurally distinguish unavailable
resolvers from immutable-source hash mismatches.

Protected evidence is defined by [protected-evidence.md](protected-evidence.md): criteria, errors, IDs, hashes, quoted evidence, security findings, and complete-read blocks remain byte-identical before any lossy line budget.
