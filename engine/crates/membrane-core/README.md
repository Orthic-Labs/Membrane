# membrane-core

The **cross-provider attention budget** owner.

This crate owns the single cross-provider attention budget whose explicit
lanes reconcile every receipt's selected and delivered tokens. Every block
carries one lane; the budget totals are the sum of the per-lane totals; the
reconciliation receipt is the single place a caller learns whether selected
and delivered tokens agree.

## Lanes

| Lane | Contract surface | Token consumption |
|---|---|---|
| `native` | host-loaded content with a matching host receipt (MBR-010) | zero bytes; zero tokens |
| `rendered` | inline text the renderer serialized into the prompt | `rendered_tokens` |
| `resolver_backed` | reference the agent retrieves by `resolver` | zero tokens; `delivered_chars` only |
| `metadata_only` | metadata without content | zero tokens; zero bytes |

The lane is derived from the block's `deliveryClass` and `deliveryMode`; lanes
are mutually exclusive per block. A block cannot appear in two lanes, and the
cross-provider total is the sum of the per-lane totals — never a separate
counter.

## Cross-provider budget

A [`CrossProviderBudget`] owns:

- the global `max_tokens` ceiling,
- one [`LaneAllocation`] per lane (the per-lane selectable cap),
- the running [`LaneAccounting`] for each lane as the packet fills.

The planner's call sequence is:

1. `CrossProviderBudget::new(max_tokens)` — the single global ceiling.
2. `budget.allocate(lane, tokens)` for each block — per-lane admission.
3. `reconcile(packet)` — produce the [`BudgetReconciliation`] that the
   receipt carries.

The reconciliation is the single receipt-side check: selected tokens per lane
must sum to the total selected, delivered tokens per lane must sum to the
total delivered, and selected must not exceed `max_tokens`.

## Layout

- `src/lane.rs` — [`BudgetLaneKind`], the per-lane allocation + accounting.
- `src/budget.rs` — [`CrossProviderBudget`] and the lane enumeration.
- `src/reconcile.rs` — [`BudgetReconciliation`] and the `reconcile` function.

## Relation to the protocol

The typed protocol shapes (`BudgetLaneKind`, `LaneAllocationV1`,
`LaneAccountingV1`, `BudgetReconciliationV1`) live in `membrane-protocol` so
both Rust and TypeScript can round-trip them. This crate builds the
reconciliation logic on top of those types.

## Verify

```sh
rightkit cargo check --manifest-path engine/Cargo.toml -p membrane-core
```

Test execution is deferred to the Book 1 gate.
