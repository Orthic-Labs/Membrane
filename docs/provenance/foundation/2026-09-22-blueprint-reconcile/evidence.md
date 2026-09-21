# Blueprint availability & refresh reconciliation

## Observed boundary

Installed unsigned Windows source `3415b0c1249b31e0bb7eaa35ba3d564aab8e4f9c`, release generation `sha256:25f8aaded6d4cb0589781cb1e1eff6a8d9ee2624885348d51da7000a48c2c559`.

Actual Codex Pull included 16 nonempty Blueprint graph blocks, labelled `stale_snapshot`. Every Blueprint source-resolution receipt was resolved with matching graph generation, overlay generation & source hash. [Raw response & assertions](stale-pull.json). Returned block accounting still uses `deliveryStage: planned` & `deliveredChars: 0`; this proof claims inclusion in actual MCP response, not a source-recovery call or corrected delivery accounting.

Earlier probe attempts exposed inherited stale bearer environment & a test agent's invented scope descriptor. Successful probe inherited installer-persisted user bearer without printing it & omitted that descriptor. Native host authenticated normally. No authorization check was relaxed.

Full installed qualification **failed** at Hub health: Membrane's repository could not join its watcher because incremental extraction rejected committed `tray.png` with `source facts unavailable`. Other two enrolled repositories had complete coverage. Sealed graph remained intact. Follow-up repair gives non-code binaries file-only facts, matching full construction; startup diagnostics retain concrete failure during retry backoff. Installed requalification of that follow-up is pending.

## Verification

- Planner: 51 passed; runtime projection/freshness/source resolution: 31 passed.
- Blueprint unit suite: 201 passed; native engine suite: 30 passed after generated-output exclusions. Subsequent binary-file regression: 1 passed.
- Query byte limits include stale-path & whole-stale cases; query suite: 10 passed. Stale provider receipts: provider suite 8 passed.
- Runtime service suite after diagnostic repair: 42 passed.
- `pnpm test`: 32 passed, 1 intentional skip; restored suite 68 passed. `pnpm test:mcp`: 95 passed.
- Canon/product docs checks passed before this receipt. No lifecycle closure claimed.

## Canon receipt rows

| Capability | State | Exact source | Exact consumer | Residual |
|---|---|---|---|---|
| PUL-027 | DELIVERED | `engine/crates/cortex-core/src/planner.rs:529` | `engine/crates/membrane-runtime/src/pull/federation.rs:1970` | COMPLETE |

## Focused verification

| Capability targets | Focused command | Direct test evidence | Result | Run identity/time |
|---|---|---|---|---|
| PUL-027 | `rightkit cargo test --manifest-path engine/Cargo.toml --locked --target x86_64-pc-windows-msvc -p membrane-runtime --lib -- pull::federation::tests source_resolution::tests freshness::tests --test-threads=1` | `native_projection_binds_ccs_to_blueprint_generation_before_resolution_gate`; `overlay_identity_uses_graph_generation_and_rejects_release_generation` | FOCUSED_PASS — 31 passed, 0 failed | RightKit `13946a92-b35f-4265-a73a-50aa4dea3355`, 2026-09-21 UTC |

PUL-027 focused verification replaces superseded stale-quarantine proof. Full acceptance qualification remains PENDING. Required/excluded scope & 0 lifecycle-closed remain unchanged.
