# Protected evidence contract v1

Lossy transforms must classify protected evidence before selection or truncation. Criteria, errors, identifiers, hashes, quoted evidence, security findings, and complete-read blocks are protected by default. Explicit `kind`, `protected`, or `completeRead` metadata protects the whole UTF-8 block; command output uses a `[complete-read]` marker. Content detection protects the exact half-open byte span, including original line ending. Empty input produces no span.

Protected bytes must reach `CompressionReceiptV1.protectedSpans` and `keptSpans` unchanged. They must never appear in `dropped`; line budgets apply only after protected spans are admitted. Span offsets are UTF-8 bytes bounded by `u32`, not characters. Compaction may redact a rendered summary, but immutable raw recovery and protected receipt spans remain byte-identical.

`tests/compression/protection-classifier.mjs` is the executable v1 classifier contract. Its adversarial fixtures cover Unicode, CRLF, NUL, criteria, failures, labeled and UUID identities, SHA-256, quoted evidence, CVE/severity findings, and explicit complete reads.

## Integration boundary

Production selection in `engine/crates/membrane/src/compression.rs` classifies before `is_signal`/head-tail selection, forces protected lines into `keep` outside the lossy budget, and copies identical byte spans into the committed `CompressionReceiptV1.protectedSpans` and `keptSpans`. No protocol change is required.
