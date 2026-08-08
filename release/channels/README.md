# Release channels and compatibility policy

`release-channel.v1.schema.json` defines `stable`, `beta`, and `nightly` as a
read-only projection. Every channel reports support state and window, schema
compatibility, migration, rollback, and signed update evidence. A null
evidence field is **unavailable**; it is never an implicit update or
permission to mutate.

## Channels and their support window

| Channel | What it is | Support window |
|---|---|---|
| `stable` | The default, promoted channel. | `supported` from promotion until the next `stable` release is promoted, then the prior release is `supported` for its documented overlap window (`support.endsAt`), then `unsupported`. |
| `beta` | Pre-release candidate for the next `stable` promotion. | `supported` for the duration of the beta cycle only; becomes `unsupported` the moment the corresponding `stable` release ships, since beta users are expected to move to `stable`. |
| `nightly` | Unreviewed, most-recent build. | `degraded` by default (no support guarantee); a specific nightly build is never individually `supported`. |

`support.endsAt: null` means the window is still open, not that support is
unlimited or unknown; `support.state` names the current claim explicitly,
including `unknown` when the state cannot be determined (never assumed
`supported` by default).

## This is enforced, not only documented

`compatibility-policy.mjs` (this directory) and
`engine/crates/membrane-protocol/src/compatibility_policy.rs` are the same
fail-closed policy, one JS mirror and one Rust source of truth. Both refuse
by default, never admit by default, on exactly:

1. **an unknown channel** — any `channel` value other than `stable`, `beta`,
   `nightly`;
2. **an unsupported schema version** — any `schemaVersion` other than the
   current `RELEASE_CHANNEL_SCHEMA_VERSION` (`1`);
3. **an unproven `schemaCompatibility`** — an explicitly `unknown` or
   `incompatible` value is refused rather than assumed `compatible`; only
   `compatible` and `migration_required` admit; and
4. **a downgrade** — a candidate `release` that is not a strictly greater
   `major.minor.patch` than the version already installed, using the same
   version-comparison rule `engine/crates/membrane-updater` (MBR-911) uses
   for its own downgrade rejection, so the two policies cannot disagree.

`node --test tests/compat/release-channel-compatibility.test.mjs` runs the
JS side today. The Rust side's `#[test]`s in `compatibility_policy.rs` are
source-verified by type and run at the Book 3 final phased gate
(`cargo test --manifest-path engine/Cargo.toml --workspace`), per this
task's no-`cargo`-execution hard rule.

Hub and CLI call sites are expected to run every reported channel descriptor
through `evaluateCompatibility`/`evaluate_compatibility` before treating it
as admitted; wiring that call site inside `apps/membrane-hub/**` is out of
this task's allowed paths (see `docs/compatibility/release-channels.md`).
