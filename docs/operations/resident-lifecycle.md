# Membrane resident lifecycle

Membrane Hub is the sole resident owner. It links `membrane-runtime` and runs the
runtime on a Hub-owned in-process thread. It never launches or adopts a Membrane
child process.

The Hub resolves the canonical workspace, claims the one-active-runtime guard,
starts the loopback service, and waits for typed readiness before reporting
Running. Hub shutdown closes admission, drains the in-process runtime, and joins
its thread. A second runtime claim is rejected before storage or port binding.

No standalone runtime command, child lifecycle pipe, lease file, supervisor PID
marker, or second supervisor binary exists. Stateless clients never auto-start
Hub; Hub absence returns typed `membrane_unavailable` with reason `hub_inactive`
and `retryable: true`.
