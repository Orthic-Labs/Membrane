# docs/evidence/

This directory holds receipts that executable code actually reads. Everything
else — historical CI output that only prose ever pointed at — has been
removed from the repo.

## Retained, load-bearing

| Path | Consumed by |
| --- | --- |
| `docs/evidence/qualification/` | `tools/productization/generate-support-matrix.mjs`, which derives `docs/support-matrix.md`, `docs/support-matrix.json`, the README support-tier block, and `server.json` platformReceipt fields from these receipts. |
| `docs/evidence/releases/` | `scripts/release/npm/resolve-platform-artifacts.mjs`, `scripts/release/oci/generate-oci-release.mjs`, and their tests. |
| `docs/evidence/platform/` | Referenced from `docs/release/platform-acceptance.md`. |
| `docs/evidence/g3/` | Kept as current-generation qualification evidence. |

Treat these subtrees as durable inputs to the tooling above — do not delete
or hand-edit their contents without updating the consuming script and
re-running it.

## Removed

`evidence/g2/` (391 files) and `docs/evidence/productization/` (257 files) were
historical CI receipts referenced only from prose in
`docs/benchmarks/vector-backend/2026-08-02-rust-vector-optimization-bakeoff-2-report.md`
and `docs/reference/deferred-surfaces.md`. No `.mjs`/`.js`/`.py`/`.rs` script,
`package.json` script, or GitHub Actions workflow read them. They were
removed from the repo to cut dead weight; the prose that cited them now says
so explicitly instead of linking to a path that no longer exists.

## Generated at run time (gitignored)

Future runs that produce receipts under `evidence/g2/` or
`docs/evidence/productization/` are gitignored (see `.gitignore`) so this history
does not reaccumulate. If a future contract needs one of those paths to
become load-bearing again, promote it deliberately: remove the ignore rule
for that path and wire a script to actually read it.
