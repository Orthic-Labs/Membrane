# Membrane install contract (MBR-203)

The Membrane binary installs under a scratch `MEMBRANE_ROOT` and only
promotes that scratch tree to the target root on `commit`. Every install is
staged, typed, and idempotent; no partial install survives an interrupted
run.

## Scratch `MEMBRANE_ROOT`

Every install runs first against a scratch directory the operator names
(`--scratch-root`). The scratch directory is the only place the install plan
mutates during stage execution. The target `MEMBRANE_ROOT` (`--target-root`)
is never touched until `commit` succeeds.

If the same scratch root already carries a committed `install-receipt.json`
for the current `plan_id`, the install is a noop — the existing receipt is
returned verbatim and no stage runs.

## Stage order

Every install plan runs the same five stages in this order. The operator
supplies the per-stage `action` and `rollback` strings in the plan JSON.

1. **Enumerate** — discover the install surface (roots, prerequisites).
2. **WriteManifest** — write the durable install manifest.
3. **MintLease** — mint the supervisor lease and sibling endpoint.
4. **PublishReceipt** — publish the install receipt.
5. **RegisterBindings** — register the bindings the runtime needs.

`action` and `rollback` are opaque shell commands (`sh -c` on POSIX,
`cmd /C` on Windows). The framework owns the staging and rollback
sequencing; the install's actual work is the operator's.

## Rollback rule

On any stage failure:

- every previously completed stage's `rollback` is run **in reverse order**;
- the receipt's `outcome` is set to `rolled_back` with the failing reason;
- the receipt is rewritten to disk with the rolled-back outcome;
- `execute_plan` returns `Err(InstallError::RolledBack { receipt })` so the
  caller can surface the failure without re-deriving it from the receipt.

`rollback` failures are recorded in `receipt.rollback_actions` so a forensic
read of the receipt shows exactly which rollbacks ran and which didn't. A
rollback failure does not abort the rest of the chain.

## Receipt on disk

`<scratch>/install-receipt.json` is rewritten after every stage — including
the initial pending state before any stage runs. An interrupted install
leaves a `pending` or `rolled_back` receipt on disk; the next run can use
that receipt to either resume (committed `plan_id`) or roll back
(rolled-back `plan_id`) without re-deriving either outcome from the state
on disk.

The receipt carries:

- `schema_version` — pinned at `1` for MBR-203.
- `plan_id` — the same id the plan supplied.
- `commit_digest` — `sha256:` of the canonical plan body.
- `started_at_unix_ms` / `finished_at_unix_ms` — wall-clock timeline.
- `stages_completed` — the stages that succeeded, in order.
- `outcome` — tagged union `{ kind: "pending" | "committed" | "rolled_back", reason?: string }`. A `rolled_back` outcome carries the failing reason; `pending` and `committed` do not.
- `rollback_actions` — the rollback strings that actually ran, in the order they ran (reverse of the forward chain).

## Idempotency

The framework is generic — it does not read the on-disk receipt to decide whether
to skip stages. Idempotency is the callback's responsibility: the operator's
`action` and `rollback` strings MUST be safe to run twice on the same scratch
root. In practice this means using flag-file markers (`mkdir -p` is idempotent,
`echo X > file` is not) and putting every side-effect under a path the
operator owns. A second install with the same `plan_id` against the same
scratch root produces a fresh receipt whose `commit_digest` ties it to the
plan that produced it.

## CLI

```text
membrane install \
  --scratch-root <scratch-membrane-root> \
  --target-root <target-membrane-root> \
  [--plan <path-to-plan.json>] \
  [--dry-run]
```

- `--plan` is the install plan JSON. When omitted, the binary executes a
  default plan with the five standard stages and `true` actions so the
  operator can hand-edit the JSON to populate the real work.
- `--dry-run` runs the plan against the scratch root and prints the
  receipt without renaming scratch to target.

## JSON plan shape

```json
{
  "plan_id": "mbr-203-tx-install-example",
  "scratch_root": "/tmp/membrane-scratch",
  "steps": [
    { "stage": "Enumerate", "action": "...", "rollback": "..." },
    { "stage": "WriteManifest", "action": "...", "rollback": "..." },
    { "stage": "MintLease", "action": "...", "rollback": "..." },
    { "stage": "PublishReceipt", "action": "...", "rollback": "..." },
    { "stage": "RegisterBindings", "action": "...", "rollback": "..." }
  ]
}
```

The same shape is shared with the JS enrollment CLI under `mcp/install.mjs`;
either side can hand the same JSON to the other.
