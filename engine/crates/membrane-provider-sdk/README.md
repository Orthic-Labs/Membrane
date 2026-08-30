# membrane-provider-sdk

`membrane-provider-sdk` is a small, publishable Rust SDK for implementing a
Membrane provider. It exposes only stable provider contracts:

- `Provider` & `CapabilityV1` for operation dispatch;
- `Fixture` & `run_conformance` for deterministic provider checks; and
- canonical JSON helpers & operation registry re-exported from
  `membrane-protocol`.

The SDK deliberately does not expose Membrane application, storage, install,
or runtime internals. A downstream provider pins compatible releases with:

```toml
[dependencies]
membrane-provider-sdk = "0.1"
membrane-protocol = "0.1"
```

See [`docs/reference/sdk/publishing.md`](https://github.com/Orthic-Labs/Membrane/blob/main/docs/reference/sdk/publishing.md)
for release metadata & compatibility policy.
