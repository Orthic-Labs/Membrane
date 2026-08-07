# F10 — tamper-evident canonical context-event history

Status: source implementation. Release/installed acceptance remains separate.

## Scope

F10 seals `context_event_log`, the canonical context telemetry ledger. Each
event keeps its existing canonical SHA-256 and gains an ordered chain record.
Fixed-size segments expose immutable chain roots; retention may remove only a
complete sealed segment and must leave a content-free receipt.

`memory_event_log` is a separate legacy compatibility ledger. It still has
direct mutation paths and is explicitly `legacy_unsealed`; F10 must not be
used to claim that every historical event store is sealed.

## Contract

- Chain domain is `membrane-context-chain-v1`.
- A row hash binds segment, ordinal, event ID, canonical event SHA-256, and
  previous global chain hash.
- Ordinals are contiguous inside a segment; global previous hashes continue
  across segment boundaries.
- Segment content identity is deterministic from ordered row hashes. A segment
  becomes immutable only when sealed.
- Verification recomputes canonical event hashes, chain hashes, ordinal/seq
  continuity, segment counts, and roots. First retained row must bind a valid
  retention receipt.
- Retention verifies first, deletes one complete sealed segment atomically,
  and writes count, seq range, removed root, prior root, and retained anchor.
- Replay of an identical event remains idempotent; conflicting replay remains
  rejected by existing canonical-event rules.

## Acceptance

Focused tests must prove deterministic append/reopen, identical replay,
payload/hash mutation detection, delete/reorder detection, segment sealing,
retention receipt verification, post-retention verification, and v21↔v22
migration behavior. Release evidence must report canonical ledger status and
legacy ledger status separately.
