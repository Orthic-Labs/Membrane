# Diagnostic bundle

Run one content-free support capture:

```sh
membrane cli doctor --bundle ./membrane-diagnostic.json
```

Set `CRYPT_DB` or pass `--db` as for existing doctor use. Bundle validates current doctor DB findings, configured port, token-file validity, build identity, and schema digests. Provider, adapter, and signature signals lacking a CLI reader are emitted as typed `unavailable`, never successful validation. It never stores prompts, source bodies, database rows, token values, doctor sample IDs, repair text, or local paths. `manifest` has exactly one SHA-256 value for each fixed `entries` key.
