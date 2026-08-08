# Team policy synchronization

`TeamPolicySyncV1` carries only tenant, team, user, generation, policy digest, offboarding, key-rotation, audit-export, & encrypted-envelope identifiers. It carries no content, keys, storage location, or transport instruction.

Scopes are explicit: tenant, team, user, & local root. Wire policy may cover only tenant/team/user; runtime rejects local-root scope. Trusted verification must confirm user-origin learning scope preservation, so team policy cannot broaden either boundary.

`TeamPolicyTrustVerifier` supplies encryption & authorization results from trusted crypto/policy integration. Callers cannot assert either result in wire data. Admission bounds IDs, scopes, nonces, & offboarding before calling trusted verification, then rejects non-increasing generations, missing encryption, or missing authority. Receipt binds policy, tenant/team, generation, envelope ID, ciphertext digest, decision, & stable snake-case reason.

Offboarding is represented by user IDs, key rotation by an opaque rotation ID, & audit export by an opaque export ID. Real crypto, key lifecycle, transport, persistence, and export remain integration gates.

## Opt-in gate (MBR-1007)

Team sync is off by default. `TeamSyncOptInV1` (`engine/crates/membrane-protocol/src/team_policy.rs`) is the only opt-in shape, & its one valid disabled value is `TeamSyncOptInV1::disabled()` — every field empty. Nothing in `TeamPolicySyncV1` can construct or flip an opt-in record; only an explicit local caller can.

`admit_team_policy_with_opt_in` (`engine/crates/membrane-runtime/src/team_policy.rs`) wraps `admit_team_policy` with three fail-closed gates checked *before* trusted verification runs: opt-in must be enabled & well-formed (`SyncNotOptedIn` otherwise), the policy's tenant/team must equal the opted-in tenant/team (`TenantOrTeamMismatch` otherwise), & the envelope's `key_id` must equal the opted-in `key_id` (`KeyMismatch` otherwise — a rotated key is only followed after an explicit local re-opt-in). None of the existing bounds/replay/encryption/authorization/user-scope guarantees are weakened; opting in only narrows what can ever reach them.

## Local persistence & audit export (MBR-1007)

`engine/crates/crypt-store/src/team_sync.rs` is the store-side complement: it owns durable opt-in state, per-tenant/team generation monotonicity across restarts, & a content-free audit log, using only tables new to this module (no learning-boundary or memory table is read or written). `commit_team_policy_admission` takes the runtime layer's `admitted`/`reason` decision but re-derives opt-in & generation monotonicity itself — mirroring how `maintenance_exec` re-derives budget/deadline instead of trusting a caller's claim — & refuses before opening a transaction or writing anything when there is no opt-in record, a tenant/team mismatch, or a key mismatch. Every attempt that passes those gates appends exactly one audit row (admitted or not); only an admitted attempt advances generation. `team_sync_audit_log` reads that ledger back — the local half of audit export; shipping it anywhere remains an explicit integration gate. No column in this schema can hold key material, a private key, or plaintext policy content.
