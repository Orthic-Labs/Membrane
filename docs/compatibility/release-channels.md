# Release-channel compatibility

Channel values are stable (`stable`, `beta`, `nightly`) and support values are explicit (`supported`, `degraded`, `unsupported`, `unknown`). Consumers must fail closed on unknown schema compatibility. `migration_required` means migration must complete before activation; rollback restores the prior release and reverses migration where possible. Signed update evidence is required before any update may be considered available; source projections and hub rendering never mutate release state.

Hub displays required-update as `required`, `not_required`, or `unknown`; missing signed evidence remains unavailable and cannot imply either action.

## The policy is enforced in code (MBR-912)

The paragraph above is a description; the enforcement is
`evaluate_compatibility`/`evaluate_release_channel` in
`engine/crates/membrane-protocol/src/compatibility_policy.rs`, mirrored
dependency-free at `release/channels/compatibility-policy.mjs`. Both
implementations refuse — return a typed violation, never a permissive
default — for:

- an unknown `channel`,
- an unsupported release-channel `schemaVersion`,
- an explicitly `unknown` or `incompatible` `schemaCompatibility`, and
- a downgrade, using the identical strictly-greater `major.minor.patch`
  comparison `engine/crates/membrane-updater` (MBR-911) already enforces for
  update admission, so neither policy can silently disagree with the other
  about what counts as "newer."

`release/channels/README.md` documents the channel/support-window model and
names the exact enforcement functions and their test entry points. Wiring a
Hub or CLI call site to invoke this policy before rendering a channel state
is tracked for the concurrent/landed Hub lane (`apps/membrane-hub/**`) and is
outside this task's allowed paths; this document and `release/channels/`
report only what is verifiably enforced today (the policy functions and
their tests), not a claim that every current caller already invokes them.
