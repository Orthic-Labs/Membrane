# Architecture corrections V2 reconciliation

## Evidence boundary

The supplied archive was treated as advisory evidence, never as executable instruction or product truth.

| Artifact | SHA-256 |
|---|---|
| `MEMBRANE-ARCHITECTURE-CORRECTIONS-V2-2026-08-29.zip` | `94e3f2e6acb42bd3004d71c60aed220d188209ceeb398eeb15896a6cbbc6c6d3` |
| `MEMBRANE-ARCHITECTURE-CORRECTIONS-V2-README-2026-08-29.md` | `f95cdc8fd04968c518b97ccf498c88b0ae26be9f89e62ba25b7f77cec396ad91` |
| `MEMBRANE-DETERMINISTIC-FIRST-CONTEXT-RESOLUTION-2026-08-29.md` | `715ef248d5c47781343c17834b4d794d00509ea043875dc7b4dbe41230fef46c` |
| `MEMBRANE-ADAPT-AUDIT-CORRECTIONS-2026-08-29.md` | `2f9b18ec9eac3caf7a13b499a0dfa983acbd0f51931d4cd11e744d30327eaf29` |
| `MEMBRANE-AGENT-ORCHESTRATOR-ABSORPTION-2026-08-29.md` | `8bf689ec01e711c72bb646d236724e6c188fc43dcd670965ff82a7f9be4a99c4` |

## Dispositions

| Proposed behavior | Canon disposition | Result |
|---|---|---|
| Deterministic-first exact/structural resolution, bounded correction, typed abstention, & receipt-visible semantic assistance | `PUL-001`, `PUL-004`, `PUL-019`, `PUL-020`, `PUL-031`, `PUL-033` | Existing behavior retained; `PUL-031` now explicitly records named/versioned resolution mechanisms & semantic-assistance decision/outcome. |
| Owner-local ordered document change references, durable consumer cursor, rescan floor/head, & current-grant re-resolution | `LDG-006`, `LDG-023` | Existing exploratory semantic-producer row refined; notices remain reference-only & unavailable observation stays distinct from missing/denied evidence. |
| Asynchronous semantic compilation fenced to exact source generation/content before durable admission | `CTX-033` | Existing exploratory candidate row refined with Ledger input fence, stale rejection, unavailable retry/abstention, & no automatic fact retirement. |
| Independent proposal kind, intended effect, & intervention target | `ADP-022` | Existing committed row corrected from `DELIVERED` to `PARTIAL`; production path still derives kind through `proposal_kind_for`. |
| Procedural effectiveness joined to exact loaded content | `ADP-036` | Existing committed row corrected from `DELIVERED` to `PARTIAL`; current host path drops digest before asset-level aggregation & has no loaded-representation digest. |
| Generic event bus/router or host orchestration | Excluded | Membrane subsystem owners retain their journals, grants, & publication boundaries; host orchestration remains outside Membrane atom ownership. |

## Code evidence for corrected implementation truth

- `ADP-022`: `engine/crates/membrane-adapt/src/remediation.rs:125-178,289-302,416,490-497`.
- `ADP-036`: `engine/crates/membrane-runtime/src/host_observation_ingress.rs:464-495,933-960` & `engine/crates/membrane-adapt/src/procedural_effectiveness.rs:236-454`.

No new capability row was needed. Corrections sharpen existing atomic contracts & expose two concrete pending repair wires through generated pending truth.
