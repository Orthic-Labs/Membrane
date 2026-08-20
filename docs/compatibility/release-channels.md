# Release-channel compatibility

`release-channel.v1.schema.json` defines `stable`, `beta`, and `nightly` as
read-only descriptors. `engine/crates/membrane-protocol/src/compatibility_policy.rs`
is source of truth; its JS compatibility test rejects unknown channels,
unsupported schema versions, unproven compatibility, and downgrades.

No Membrane desktop renderer or updater consumes these descriptors. Release
channel state is deferred under S-11, and action transport is deferred under
S-12 in `docs/reference/deferred-surfaces.md`. Membrane Hub may consume either
only under an explicit Hub contract; Membrane does not infer install or update
policy from channel data.
