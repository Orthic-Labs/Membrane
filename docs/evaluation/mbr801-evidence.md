# MBR-801 evidence verifier

`scripts/qualification/verify-mbr801-evidence.mjs` validates explicit Mac and Windows installed receipts only. It requires exact expected commit and release generation, passed installed execution, all 10 canonical scenarios with unique trace IDs, a complete benchmark, and readable archived receipt paths for each trace.

It performs no client or service invocation and emits only content-free JSON summary. Missing, stale, incomplete, duplicate, or unarchived evidence stays `open`; a source-ready result still needs real-host receipts supplied for both platforms.
