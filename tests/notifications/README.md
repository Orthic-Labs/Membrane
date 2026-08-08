# MBR-711 notification contract

Runtime coverage exercises thresholded persistence, flap deduplication,
explicit recovery, unknown-as-unavailable handling, bounded retention, and
serde restart round-trips. Malformed or out-of-order evidence is ignored.
Receipts contain identity and non-empty evidence metadata, never notification
content; provider/dimension dedupe keys are unambiguous.
