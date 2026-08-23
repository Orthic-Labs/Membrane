# Membrane native-runtime migration ledger

This directory freezes Membrane's executable federation boundary at baseline
`322855c33e65dc936e3927570451c98e54fb0bd2` for MEM-000. It is migration input,
not runtime configuration.

## Scope

`executable-ledger.json` records shipped/runtime artifacts and every discovered
production launch or loopback site. Python federation gateway/providers are
`port` rows owned by MEM-014, MEM-017–026, or MEM-027, then deleted by MEM-030.
The Python worker bridge is deleted by MEM-029. Existing Hub, MCP, Blueprint,
Cortex, and Guide boundaries remain typed owner boundaries.

Tests, benchmarks, evaluation runners, release helpers, and unrelated workspace
tooling are development-only inputs. Their parity responsibilities are named
by fixture or shadow packets; they are not production launch edges.

## Locked ordering and ownership

Legacy gateway order is `blueprint, audit, architect, cortex, git, live, rules,
anchors, skills`. Native order is `anchors, blueprint, rules, live_files, git,
audit, architect, skills, cortex`, as locked by MEM-010. Candidate IDs use
first-wins deduplication after native canonical ordering. One monotonic deadline
flows from ingress through freshness, provider work, merge, and publication.

Blueprint owns repository truth and remains daemon/protocol-only. Cortex owns
durable memory and remains application-API-only. Guide owns its rebuildable
document index. Membrane catalog owns grants, generations, events, and
content-free receipts. No provider opens another owner's SQLite store.

## Nine-lane contract

`federation-contract-inventory.json` records each lane's input, output, terminal
errors, generation seal, trust class, provenance, ordering, parity fixture, and
cutover gate. Optional provider failures degrade locally with attributable
omissions; invalid request, scope, hard prerequisite, planner, or serialization
failures remain request failures. Public V1 shapes do not change.

## Validation

Arrays are lexically ordered by stable ID/path. `complete` is true only with no
unknown disposition, blocker, unowned launch site, or ownership collision.
Static packet checks must verify baseline, owned paths, diff whitespace, and
absence of changes outside these six files. MEM-001 seals behavior fixtures;
MEM-I01 independently reproduces coverage and acceptance.
