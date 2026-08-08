# Sparse notifications (MBR-711)

`membrane_runtime::notifications::NotificationState` tracks one bounded entry
per unambiguous length-prefixed provider/dimension key. Incomplete evidence is
ignored. Unavailable, degraded, or unknown evidence counts
as failure; no liveness or health inference is performed. `threshold` failures
within `window_seconds` emit one content-free alert. Repeated observations and
flaps are deduped. A resolution is emitted once, only after explicit healthy
evidence carrying source, evidence, and resolver fields. State is serde
serializable for restart persistence; stale out-of-order samples are ignored;
oldest observed entries leave first when `capacity` is reached.
