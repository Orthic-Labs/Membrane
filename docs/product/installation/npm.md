# npm bootstrap

> See also: [installation reference](README.md) for the manifest/IPC handshake contract and stable-roots reference.

`@membrane/membrane` is a thin loader for a native Membrane command/service
pair on macOS. It selects its Darwin platform package, validates artifact bytes against its
recorded digest and signature metadata, then exposes native dispatch. Missing
metadata, unsupported hosts, digest drift, or failed signature verification
fail closed before dispatch.

Platform packages are not published from this checkout. A future package must
bind to externally produced sealed Membrane release-generation evidence; the
pure `scripts/release/identity.mjs` helper validates release IDs without
creating, sealing, uploading, or publishing a release record.

Membrane native distribution owns desktop installation & cleanup; shared release
tooling owns native packaging & publication. This package is headless transport only; it never creates an
external product manifest or delegates install authority to another product.
