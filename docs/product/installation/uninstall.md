# Membrane ownership-safe uninstall (MBR-205)

Uninstall refuses to remove anything not owned by a Membrane receipt. The
contract is symmetric with MBR-203's transactional install: every path the
install wrote is recorded in an ownership table, and the uninstall reads
that table before touching anything on disk. A fresh install after an
uninstall is always permitted because the new install writes its own
ownership table.

## Ownership model

Every path the install touches outside the four stable roots is recorded
in `<receipt_root>/ownership.json` as one `OwnershipClaim` row with a
kind tag:

| Kind      | What the runtime wrote                              |
| --------- | --------------------------------------------------- |
| `Manifest`| The durable install manifest                        |
| `Lease`   | The supervisor lease and sibling endpoint           |
| `Receipt` | The install / uninstall receipts themselves         |
| `Binding` | Native MCP client entries, watcher hooks, registry  |

The table is a flat JSON document:

```json
{
  "installationId": "sha256:<hex of receipt root>",
  "claims": [
    {
      "kind": "Manifest",
      "path": "/var/membrane/manifest.json",
      "receiptId": "plan-1",
      "registeredAtUnixMs": 1755000000000
    }
  ]
}
```

`installationId` is `sha256:<hex>` of the receipt root, so two installs
at different paths cannot accidentally share a table.

## Refuse-by-default rule

`revoke_unowned(table, candidates)` returns the subset of `candidates`
that ARE in the ownership table. Anything not in the table is left
alone — the caller is expected to log a warning so the operator sees
which paths were refused. `execute_uninstall` never calls the `remove`
callback for an unauthorised path; a stray `--candidate` cannot delete
user data even if the operator typed the wrong flag.

When the ownership table is missing entirely (a fresh checkout, a
post-reset install, a corrupt receipt) the table is treated as empty and
every candidate is refused. The receipt records `RefusedAll` so the
audit trail shows the refusal was deliberate.

## Refusal receipt shape

`execute_uninstall` returns a typed `UninstallReceiptV1`:

```json
{
  "schemaVersion": 1,
  "installationId": "sha256:<hex>",
  "startedAtUnixMs": 1755000000000,
  "finishedAtUnixMs": 1755000001000,
  "removed": ["/var/membrane/manifest.json"],
  "refused": ["/var/membrane/stray.txt"],
  "outcome": "completed"
}
```

`outcome` is one of:

- `pending` — the run started; no decision is final yet.
- `completed` — every authorised candidate was removed.
- `refusedAll` — every candidate was refused; nothing was removed.
- `partiallyRemoved` with a `reason` — at least one authorised remove
  failed; the receipt captures what was removed before the halt so a
  retry can resume.

The CLI persists the receipt as `<receipt_root>/uninstall-receipt.json`
alongside the ownership table so a forensic read sees both records in
one place.

## Idempotency

A re-run is a no-op against the refused set: paths the previous
uninstall already removed are no longer on disk, so the new run's
`revoke_unowned` returns the empty set for them and they appear in
neither `removed` nor `refused`. The receipt's `startedAtUnixMs` /
`finishedAtUnixMs` differ across runs, but the `installationId` is
stable so the audit trail can be correlated.

## Interaction with MBR-203

MBR-203 writes `install-receipt.json` and atomically renames the scratch
root to the target root on `commit`. The matching ownership table is
written by the install stages as part of the same plan. After an
uninstall:

- The install receipt is removed (it is in the table as `Receipt`).
- The manifest, lease, and binding rows are removed.
- The `ownership.json` table is left in place so a forensic read sees
  what was claimed.

A fresh install after an uninstall writes a new ownership table
because the install plan starts from a fresh scratch root. The new
table's `installationId` matches the receipt root, so the two records
align. There is no "previously-installed" state the uninstall must
clean up; the only requirement is that the scratch root the install
starts from is empty, which `commit` enforces by refusing to clobber
an existing target root.

## CLI

```text
membrane uninstall \
  --receipt-root <receipt-membrane-root> \
  [--candidate <path>]... \
  [--dry-run]
```

- `--receipt-root` is the directory containing `ownership.json` and
  `install-receipt.json`. A missing table is treated as empty.
- `--candidate` may be supplied multiple times. Anything not in the
  table is refused.
- `--dry-run` prints the authorised set as JSON without removing
  anything. The refused set is echoed alongside so the operator sees
  what would be left alone.

## JS mirror

`mcp/install.mjs` exports the symmetric `authorizeUninstall` helper and
`OwnershipTable` shape so the JS enrollment CLI refuses the same
candidates the Rust binary would refuse. The contract is identical: a
missing table is empty, a duplicate `(kind, path)` pair surfaces
`DuplicateOwnershipError`, and the `installationId` is `sha256:<hex>`
of the receipt root.
