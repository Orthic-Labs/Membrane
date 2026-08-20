# Membrane Continuity

Canonical Claude & Codex transcript normalization lives under
`continuity.transcript`. It emits deterministic `TranscriptEventV1` events,
source byte spans, parser receipts, & typed `TranscriptUnavailable` failures.

Missing or inaccessible transcripts raise typed failures; callers must not
turn omission into an empty-success result.
