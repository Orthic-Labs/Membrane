# Membrane resident lifecycle

Membrane Hub is sole resident owner. It launches `membrane supervisor-child` with inherited
stdio, sets `MEMBRANE_LIFECYCLE_STDIO=1`, & retains stdin/stdout for lifecycle frames.

Hub sends one closed `ResidentHelloV1` frame. It binds product identity, canonical workspace root,
installation digest, cryptographic instance ID, release generation, executable digest, capability,
& monotonic fence. Resident validates every binding before startup, emits `starting`, then emits
`ready` with exact fence & loopback endpoint. Commands require exact fence & capability. EOF is
typed `ParentEof` & drains admission.

No lease file, lease argv, supervisor PID marker, environment fallback, or second supervisor binary
exists. Headless `membrane supervisor-child` remains available without inherited stdio for local
diagnostics; Hub launches authenticated stdio mode only.
