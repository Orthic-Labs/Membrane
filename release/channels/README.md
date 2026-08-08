# Release channels

`release-channel.v1.schema.json` defines `stable`, `beta`, and `nightly` as a read-only projection. Every channel reports support state and window, schema compatibility, migration, rollback, and signed update evidence. A null evidence field is **unavailable**; it is never an implicit update or permission to mutate.
