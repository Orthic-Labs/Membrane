# Membrane competitive comparison

Corrective Foundation Stage 3 receipts for all 335 capability rows at `30b3c211ae874f369bed3fe92eb94b2fc5acbb16` against frozen 27-repository corpus in `../2026-08-31-implementation-comparison/corpus.json`.

## Result

| Subsystem | Current best / closed | Donor better | Current incomplete | Unresolved | Not committed | Total |
|---|---:|---:|---:|---:|---:|---:|
| Membrane | 12 | 0 | 24 | 30 | 0 | 66 |
| Pull | 0 | 1 | 19 | 18 | 1 | 39 |
| Push | 7 | 0 | 17 | 0 | 0 | 24 |
| Cortex | 10 | 3 | 16 | 8 | 1 | 38 |
| Blueprint | 20 | 18 | 20 | 10 | 1 | 69 |
| Ledger | 7 | 4 | 12 | 4 | 1 | 28 |
| Adapt | 10 | 2 | 46 | 6 | 7 | 71 |
| **Total** | **66** | **28** | **154** | **76** | **11** | **335** |

Only committed `CURRENT_BEST` rows are competitively closed. `DONOR_BETTER`, `CURRENT_INCOMPLETE`, & `UNRESOLVED` rows generate pending work. `NOT_COMMITTED` rows remain exploratory & outside committed progress.

Lifecycle implementation, verification, qualification, delivery, & acceptance evidence remain independent. Competitive closure never fabricates release or installed qualification.

## Receipts

- [Membrane](membrane.md)
- [Pull](pull.md)
- [Push](push.md)
- [Cortex](cortex.md)
- [Blueprint](blueprint.md)
- [Ledger](ledger.md)
- [Adapt](adapt.md)
