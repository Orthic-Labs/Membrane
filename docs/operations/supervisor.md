# Supervisor — per-user Membrane supervisor

**MBR-201** introduces the `membrane-supervisor` binary and the
`engine/crates/membrane-supervisor` crate. The supervisor is a separate OS service that
sits in front of the `membrane` binary. It runs as the user (per-user launchd agent on
macOS, per-user systemd unit on Linux, per-user Task Scheduler entry on Windows), owns
exactly one resident process, and dedupes the cortex watcher.

## Why a separate process

The supervisor and the resident are *different* OS processes and never collapse into
one. Three reasons drive that split, all anchored in the concrete failure modes we
have seen on real installs:

1. **The supervisor must outlive a resident crash and respawn it without holding the
   engine database open.** A single-process design would force the engine to close and
   reopen the SQLite handle on every crash; that is exactly the path that loses
   write-ahead-log frames.
2. **The supervisor must be in OS service management.** launchd / systemd / Task
   Scheduler each give us a per-user, restart-aware runtime that already handles
   priority, logging, and crash reporting. Baking that in is more code than the
   supervisor's actual logic.
3. **The resident must own the loopback port; the supervisor must not.** If the
   supervisor bound the port itself, every client would have to authenticate against
   the supervisor's authority instead of the resident's `installation_manifest`. That
   breaks the contract defined in `docs/installation/contract.md`.

## Shape

The supervisor's footprint on disk:

```
~/.membrane/supervisor/
├── supervisor.pid        # single-instance lock (one supervisor per user)
├── lease.json            # lease handed to the resident via `MEMBRANE_SUPERVISOR_LEASE`
├── endpoint.json         # discovery file client adapters read
├── status.json           # most recent in-memory state of the supervisor
└── supervisor-state.json # durable issuance counter + instance id
```

The supervisor writes each of these with atomic temp + rename so a half-written file
cannot be parsed by the resident or by a client.

## Loopback port ownership

The supervisor publishes the loopback port in `endpoint.json`. Clients — stdio MCP
adapters, the Hub, the CLI — read it once and connect. The supervisor never binds the
port itself; the resident does. The supervisor's only network interaction is asking the
OS whether a recorded PID is alive (`kill(pid, 0)` on Unix).

## Lease lifecycle

1. **Mint** — the supervisor builds a `SupervisorLeaseV1` every outer-loop iteration.
   `supervisorInstanceId` is stable across restarts; `issuance` increments.
2. **Publish** — the supervisor writes the lease to `leasePath` atomically. A temp file
   is created and then renamed; a partial write cannot be acted on by the resident.
3. **Hand** — the supervisor spawns `membrane supervisor-child --lease <path> --loopback-port <n>`.
   The runtime reads the lease, hashes its content, and refuses to start if the lease's
   `contentDigest` does not match.
4. **Refresh** — when the resident exits, the supervisor mints a fresh lease with
   `issuance + 1`. The new resident validates the new lease. Clients holding the
   previous `endpoint.json` see the change on their next poll.

## Watcher dedup

The cortex watcher is a separate sidecar (`cortex-watch.mjs`) that subscribes to FSEvents
/ inotify / ReadDirectoryChangesW. Two watchers would each open their own file
descriptors on the same workspace roots, doubling CPU and producing duplicate events.

The supervisor coordinates via the watcher pidfile:

| Recorded state | Script present | Supervisor action |
|---|---|---|
| Live PID | yes | `Adopt` the existing watcher. Do not spawn. |
| Dead / missing PID | yes | `SpawnFresh`. |
| (n/a) | no | `Unavailable`. Surface as `unavailable`, do not spawn. |

Two supervisors running at the same time cannot both spawn because the supervisor's
PID lock guarantees only one supervisor owns a given lease per user. If a new
supervisor emerges after the old one dies, it reads the watcher pidfile, sees the
recorded PID is alive (because the watcher outlived the supervisor), and adopts.

## Single-instance enforcement

The supervisor's per-user PID lock is at
`<config.pidLockPath>`. The lock manager:

1. Reads the recorded PID, if any.
2. Probes liveness with the platform-appropriate call.
3. If live AND foreign, returns `Held { pid }`; the supervisor refuses to start.
4. If dead, missing, or unparseable, atomically rewrites the lock with the supervisor's
   own PID.

This is a deliberately conservative fail-open: a single stuck or unkillable process can
prevent a new supervisor from starting. That is the correct outcome — a stuck process
holding the lock is a real problem the user needs to see.

## Restart policy

The supervisor's restart policy is configured in `install/config.example.json`. It
applies only to the resident (the OS service manager restarts the supervisor itself):

```
restartPolicy {
  maxRestarts: 5
  windowSeconds: 60
  initialBackoffSeconds: 1
  maxBackoffSeconds: 30
  backoffMultiplier: 2
}
```

A backoff schedule for the resident looks like: 1s → 2s → 4s → 8s → 16s → 30s → 30s →
… → 30s. Once `maxRestarts` exits happen inside `windowSeconds`, the supervisor stops
restarting and surfaces the failure to the OS service manager. The manager's own
`Restart=on-failure` then takes over.

## Client reuse

The acceptance criterion for MBR-201 is "Multiple clients reuse one healthy service and
duplicate watchers are impossible." The reuse path is `endpoint.json`:

```json
{
  "schemaVersion": 1,
  "supervisorInstanceId": "sup-...",
  "leaseIssuance": 7,
  "loopbackPort": 47851,
  "leasePath": "/Users/.../supervisor/lease.json",
  "mintedAt": "2026-08-07T12:00:00Z"
}
```

Every client reads the same file, sees the same `loopbackPort`, and connects to the
same resident. When the supervisor respawns the resident the file's `leaseIssuance`
advances; clients that present a stale value get the same `loopbackPort` (the new
resident binds it within milliseconds) and a fresh handshake via
`X-Membrane-Manifest`.

## Integration with MBR-102 / MBR-105

- **MBR-102** (`f700e8b`) introduced the `membrane` binary with four modes: `cli`,
  `stdio-mcp`, `loopback-api`, and `supervisor-child`. The supervisor uses the
  `supervisor-child` mode exclusively.
- **MBR-105** (`e4ea9fd`) introduced the `InstallationManifestV1` handshake. The
  resident publishes the manifest before opening the loopback port; clients send their
  own copy on every request. The supervisor's `endpoint.json` is independent of that
  manifest — the manifest is resident-to-peer identity, the endpoint is
  supervisor-to-discovery.

## Test surface

The `engine/crates/membrane-supervisor` crate ships with the following test modules:

| Module | Surface | What it proves |
|---|---|---|
| `lock` | `SupervisorLock::try_acquire`, `release_if_owned` | Single-instance enforcement; stale-PID reclaim. |
| `watcher` | `decide_action`, `WatcherCoordinator::decide_with_invariant` | The four-case truth table; duplicate-spawn impossible. |
| `lease` | `SupervisorLeaseV1::verify_self_integrity`, `publish_lease` | Tampering surfaces; atomic write. |
| `supervisor` | `Supervisor::publish_lease`, `dry_run`, `should_restart` | Loopback port reused across cycles; restart policy is monotonic. |
| `resident` | `ResidentInvocation::argv`, `preflight_resident_binary` | Resident is always invoked with `--lease`. |

The tests compile but **do not run** at task-committed time; they exercise at the
Book 1 gate along with every other deferred command.

## See also

- `docs/installation/contract.md` — the IPC handshake contract that the resident
  publishes and clients present.
- `install/macos/com.membrane.supervisor.plist` — the per-user launchd agent.
- `install/linux/membrane-supervisor.service` — the per-user systemd unit.
- `install/windows/membrane-supervisor.xml` — the Task Scheduler entry.
- `install/config.example.json` — canonical supervisor config.
