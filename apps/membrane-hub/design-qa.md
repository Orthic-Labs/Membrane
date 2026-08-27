# Membrane Hub design QA

**Findings**

- No actionable P0/P1/P2 findings remain after final comparison.

**Source visual truth**

- Selected reference: `C:\Users\adrds\.codex\codex-remote-attachments\01a0429b-43c1-7003-8a9b-3dcc28f1855f\4F68368F-0B4A-4D99-9236-12A6B07CA8B7\1-Photo-1.jpg` (1280 x 1024 px composite; Overview board plus Ledger, Sources, Subsystems drill-downs).
- Signed source layout: `D:\Claude\membrane\docs\design\hub-redesign\dashboard.html`.
- Brand constraints: `D:\Claude\membrane\docs\design\hub-redesign\MEMBRANE-BRAND-IDENTITY.md`.

**Implementation evidence**

- Overview: `.cache/qa/overview-1280x800-final.png` (1280 x 800 px, CSS 1280 x 800, device scale 1).
- Ledger: `.cache/qa/ledger-1280x800-final.png` (1280 x 800 px, CSS 1280 x 800, device scale 1).
- Sources: `.cache/qa/sources-1280x800-final.png` (1280 x 800 px, CSS 1280 x 800, device scale 1).
- Combined source + implementation comparison: `.cache/qa/comparison-overview-final.png` (2560 x 1100 px capture; source and implementation placed side by side at equal display scale).
- Capture surface: Windows Edge desktop headless capture at 1280 x 800; no browser chrome included; no density normalization required for implementation captures.

**State & interaction coverage**

- Overview captured with running resident, live snapshot, 24-hour admission window, admitted 9, withheld 3, typed omissions `cross_root: 1` and `budget_exhausted: 2`, budget pressure 2 as withheld subset, unavailable row-level receipts, and typed subsystem states.
- Ledger captured with hash-routed selected state, working subsystem/verdict/search filters, truthful aggregate admission summary, and explicit row-level empty state.
- Sources captured with hash-routed selected state, bounded provider evidence, full `transport_unavailable` readiness reason, unknown generation/parser fields, and explicit contribution empty state.
- Subsystems, Memories, and Sessions are hash-routed projections over same snapshot model.
- Browser-contract checks cover selected navigation markers, `hashchange`, render projections, Canvas admission chart, semantic progress bars, startup-owner removal, and no fabricated reference counts.

**Full-view comparison evidence**

- `.cache/qa/comparison-overview-final.png` shows source and implementation together. Shared structure is preserved: dark flat shell, 208px rail, 40px titlebar, sidebar-column title, fused active navigation, compact summary region, dense bordered panels, purple action links, and semantic green/amber/red verdict language.
- Implementation intentionally replaces source's fabricated multi-bucket trend with truthful admitted-vs-withheld Canvas ring, typed omission bars, subsystem state rows, and unknown states where snapshot lacks evidence.

**Focused comparison evidence**

- Individual final captures were opened at native 1280 x 800 for nav/titlebar geometry, Overview boundary Canvas + omission bars, Ledger filters + aggregate summary, and Sources readiness metadata. Text, state shapes, and reason strings are readable at capture scale; no additional crop was required.

**Comparison history**

1. Initial capture identified P1 issues: body headings used Tanker outside wordmark/titlebar, synthetic near-epoch timestamps were visible, omission bars used one color, Sources readiness reason clipped, and empty Ledger/Sources panels were oversized. Fixes: body UI typography, invalid/near-epoch time suppression plus realistic capture timestamp, typed omission colors, stacked readiness reason, compact panel sizing, and read-only rail boundary/live status.
2. Follow-up capture (`overview-1280x800-v2.png`, `ledger-1280x800-v2.png`, `sources-1280x800-v2.png`) confirmed visual fixes, but sparse panel sizing plus source footer clock formatting were refined once more.
3. Final capture (`overview-1280x800-final.png`, `ledger-1280x800-final.png`, `sources-1280x800-final.png`) was inspected individually and against combined comparison. No actionable P0/P1/P2 differences remain.

**Verification**

- `node --check src/overview.mjs` passed.
- `node --check src/shell.mjs` passed.
- `pnpm exec node --test tests/overview-tabs.mjs tests/overview-dashboard.mjs tests/hub-chain.mjs` passed: 18/18.
- `pnpm exec node --test tests/cache.mjs ...` dashboard-owned checks passed; unrelated native `src-tauri` architecture checks remain owned by other work.
- Static browser-contract assertions passed.

**Implementation Checklist**

- [x] Preserve HeardRight shell geometry and fused selected navigation.
- [x] Keep Tanker limited to wordmark, sidebar title, and titlebar title.
- [x] Use truthful snapshot-derived metrics with explicit unknown/empty states.
- [x] Use Canvas for admission chart and semantic HTML progress bars for typed omissions.
- [x] Remove dashboard startup ownership and show read-only boundary/live rail status.
- [x] Implement functional Overview, Ledger, Sources, Subsystems, Memories, and Sessions routes.
- [x] Inspect final Windows desktop captures at same 1280 x 800 viewport.

**Follow-up Polish**

- P3 only: when Hub begins emitting row-level decision receipts, Ledger will naturally replace aggregate-only empty state with live rows.

final result: passed
