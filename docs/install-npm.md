# npm bootstrap (MBR-906)

`@orthic/membrane` is a thin loader. It maps `process.platform` + `process.arch` to a per-platform native package, validates artifact bytes against `metadata.sha256`, validates signed metadata bound to that digest, then exposes native `dispatch`.

The package contains no Node server core. Unsupported tuples, missing metadata, digest mismatch, invalid signature metadata, failed injected signature verification, or native packages without `dispatch` fail closed before dispatch. `npm/index.mjs` mappings are fixture/template names only; no package is published by this source change.

Consumers provide artifact bytes, release metadata, a loader, and (when cryptographic verification is available) `verifySignature({ artifact, digest, signature })`. Registry publication, native artifact production, signing-key trust, and install acceptance remain release gates.
