# Master atom intake reconciliation

## Frozen inputs

- Archive: `D:/Downloads/master_atom_list.zip`
- Archive SHA-256: `5413F1DF0EA80C8A335098E6CA7E9BD23234CA1DCF8DE63B5700E1DA6E88D82D`
- Archive target revision: `29adfc8e2fe5a2d43ed25634a91ebec3bb4070d3`
- Current comparison revision: `a9a4afb3eeaf4ee00869e8c303c50f810632f273`
- Method: six isolated SOL-high subsystem inventories, then two independent SOL-high cross-subsystem candidate challenges.

| Subsystem | Report SHA-256 | Rows | Existing | Proposed new | Register | Duplicate | Excluded | Unresolved |
|---|---|---:|---:|---:|---:|---:|---:|---:|
| Blueprint | `BF3CB788149B4A1BF472000E83E7613DC9B68E9AA38C7ACB21446A374A771A69` | 60 | 37 | 1 | 14 | 2 | 6 | 0 |
| Pull | `4ABC727E69B759D203B64B520F3443775952AC0D3624967D6DB0C1FFD5951A08` | 41 | 36 | 3 | 1 | 0 | 0 | 1 |
| Push | `D9AEF4506F43DD393CD08541ADA3318FF72DD71255D58BC85D8483879685359C` | 54 | 26 | 12 | 10 | 1 | 5 | 0 |
| Cortex | `EF6AE0F3887502E4D912DECB841316B4F2101368317A20AC6B8105290A3E1B66` | 60 | 37 | 1 | 8 | 0 | 14 | 0 |
| Ledger | `D669D811FB3BF0086758C3F709FF522E9A4612C794A64A80BE63CD48415A8D47` | 68 | 47 | 1 | 16 | 2 | 2 | 0 |
| Adapt | `C47168F06DB3A3B20C1A584D17ED5F8F7D44F4ADF6179C5C0AC72E18F751A415` | 53 | 39 | 3 | 3 | 0 | 8 | 0 |
| Total | — | 336 | 222 | 21 | 52 | 5 | 35 | 1 |

## Reconciled atom split

Two independent central challenges proposed 11 & 7 new identities. Stricter Foundation split won: shared state, caller, failure semantics, & one observable outcome remain together; implementation mechanisms stay under existing atoms.

| Accepted ID | Observable gap |
|---|---|
| PUL-037 | Bounded same-session unchanged-evidence suppression with typed omission & deterministic recovery. |
| PUL-039 | Versioned byte-stable reusable packet prefix without policy or evidence-membership drift. |
| PUL-040 | Semantic placement classes without membership, authority, rank, or atomic-group changes. |
| PSH-024 | Exact bounded partial restore from opaque recovery anchor. |
| CTX-038 | Truthful `exact`/`lower_bound` result completeness with machine-readable causes & counts. |
| ADP-072 | Evidence-bound clarification persistence, answer binding, & same-lineage resume. |
| ADP-073 | One apply-eligible pending proposal per semantic target & target version. |

All seven are COMMITTED, missing implementation, & projected into generated pending index. User supplied explicit promotion authority on 2026-08-31.

## Reclassified candidates

- Blueprint architecture-fitness bundle maps to BPT-041, BPT-048, & BPT-049–052 registers/qualification.
- Pull archive PUL-038 bundle maps to existing Ledger expansion & representation registers.
- Push command reducers, exemptions, fidelity classes, structured/log codecs, lossy summaries, embedded spans, repeat dedupe, & final-cost gate map to existing Push/Pull implementations, qualifications, or planner ownership. Only partial restore becomes PSH-024.
- Ledger checkpoint/resume maps to LDG-017, LDG-018, & MEM-044 as scale-contingent implementation/qualification work.
- Adapt clarification source rows AFA-009 & AFA-028 merge into ADP-072.

Result: 7 promoted identities; 14 proposed-new source rows reclassified; 0 unresolved.

## Current-truth corrections

- PSH-002 is PARTIAL/STALE: current `/expand` restores stored bytes but does not call existing marker source-digest verifier.
- PSH-003 is PARTIAL/STALE: source skeletonization exists; structured inputs remain exact-copy without qualified structured/log codecs.

## Reuse boundary

Direct reuse is allowed only where subsystem report verifies compatible license & exact mechanism. GitNexus noncommercial evidence remains reference-only. Unresolved or unverified donor mechanisms require behavioral reimplementation.

Final intake accounting: requested 336; evaluated 336; unresolved 0; excluded 35.
