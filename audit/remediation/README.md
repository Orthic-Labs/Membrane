# Semantic audit & dogfood remediation

Active scope: validate PR #30, reproduce & repair confirmed findings on main, qualify installed Windows product & affected public boundaries, reconcile recoverable branches, commit & push. No product closure is claimed.

## Merge & evidence validation

- Main/origin baseline: `010bb010d0c454749dd129e6952731b6417f0d26`. PR #30 merged `9420efe144b933fd0b54f56d4b26d15593bb3a3d`; ancestry succeeds & source-tip/main tree diff is empty.
- All 42 committed-byte artifact hash comparisons pass, including Session A initial/final artifacts & Session B manifests. Checkout line endings are not substituted for committed bytes.
- Requested original baseline: `75c8427437a385ef78c5805624d139741fb5f1b1`. Both artifacts instead name `a4e0a9e02c64e5439cdec289a160b748f5e5686c`, claiming user supersession. Delta: 101 files, 17,162 insertions, 2,060 deletions. Original user correction/transcript & Session A pre-B freeze chronology remain unverified. Self-attestation is not independent provenance.
- Session A matrix has 353 rows; denominator alone does not prove per-atom terminal coverage. Semantic trace validation remains open.
- Session B reports zero executed product cases. Its frozen oracle is a test specification, not an independent completion PASS. Native test labels exceed actual assertions: F01 never interrupts/corrupts storage; H01 never prepares/resolves; I01 never revokes mid-flight; J01 omits lifecycle; L02 never compares adapters. Artifact identity & isolated tray host are not enforced by environment-variable presence. Preserve original artifacts; repaired harness must use separate files.
- Main CI run `34106940481` fails runtime-language manifest check: stale Cortex digest & seven unclassified developer/audit paths.

## Remediation ledger

| ID | Finding | State | Acceptance |
|---|---|---|---|
| SEM-001 | Installed Hub-off explicit execution unreachable | Blueprint Node graph/build/refresh tests pass; native installation pending | Every explicit subsystem operation remains available; only automatic work requires Hub |
| SEM-002 | Unbound audit/decision providers claim complete-empty | Repaired in `c28e4f72`; focused checks & independent source review pass | Missing owner becomes typed omission & required lane remains insufficient |
| SEM-003 | Workspace child deadline resets & serial fanout | Source validation pending | One ingress deadline, bounded concurrency, healthy siblings retained |
| SEM-004 | Push recovery lacks task/session binding | Source-confirmed; open | Same-task restore succeeds; wrong task/session denied |
| SEM-005 | Diagnostics audit persistence silently fails | Source validation pending | Typed caller-visible persistence degradation |
| SEM-006 | Discovery readiness absent | Source validation pending | Stable schemas with truthful operational readiness |
| SEM-007 | Warm Pull composition repeatedly initializes owners | Unmeasured opportunity; open | Production counters & revocation-preserving reuse |
| SEM-008 | Generated budget claims exceed evidence | Source-confirmed; open | Inventory distinguished from qualified behavior |
| EVID-001 | Baseline substitution & independence chronology unproven | Open | Raw authoritative turns & freeze timeline |
| EVID-002 | Dogfood test assertions do not cover frozen oracle | Source-confirmed; open | Every required observable asserted through correct boundary |
| CI-001 | Runtime inventory drift blocks merged main | Inventory regenerated & local checks pass; current native CI pending | Manifest/graph checks & managed CI pass |

## Process degradation

Installed stable Legion `contract seal --help` returns `status: incomplete`, `native contract implementation is not connected`; `run open --help` returns top-level usage. No contract receipt is claimed. Direct ambient source/evidence work continues; locked-domain effects remain unexecuted.

## Delivery state

SEM-002 source repair verified & independently reviewed. Main commits `c28e4f72`, `ae26527b`, `0da3b013` & `0fcc3091` pushed from Windows using configured GitHub keyring. No local Rust build/test, installed activation, branch deletion or overall completion claim. Native CI on `0da3b013` compiled but failed fixture junction creation; `0fcc3091` exposed misplaced MCP HTTP executor wiring, corrected in next candidate. Unsigned Windows installer run `34125993668` succeeded for earlier Blueprint-only candidate; broader boundary must pass before activating final build.

## Urgent internal installer scope correction

Adrian requires latest installed Blueprint before CodeRight resumes; unsigned internal installer explicitly authorized. This task owns local activation & uses existing `unsigned-installer` managed workflow. Default native/retained discovery includes Blueprint for clients without custom tools/list metadata. MCP resource transport returns valid contents or JSON-RPC errors. Native compile/test & installer build remain managed CI work.

User expanded lifecycle correction to all six Membrane subsystems: explicit operations work with Hub off; only automatic/resident processes require Hub. Canonical contract is `docs/architecture/execution-lifecycle-boundary.md`. Blueprint source tests cover initialization, traversal, edit/refresh, build, generation rejection & no watcher. Windows checkout CRLF broke hashed Python provider; LF byte preservation restores existing expected checksum without bypassing validation. Shared explicit execution now covers Cortex, Pull, Ledger, Adapt & Push. Diagnostics cross-process state uses canonical event storage; provider children stop after explicit acquisition. Native CI & final installed acceptance remain tracked independently.
