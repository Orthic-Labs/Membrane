# npm bootstrap

> See also: [installation/](installation/) for the manifest/IPC handshake contract and stable-roots reference.

`@orthic/membrane` is a thin loader for a native Membrane command/service
pair. It selects its platform package, validates artifact bytes against its
recorded digest and signature metadata, then exposes native dispatch. Missing
metadata, unsupported hosts, digest drift, or failed signature verification
fail closed before dispatch.

Platform packages are not published from this checkout. A future package must
bind to externally produced sealed add-on release-generation evidence; the
pure `scripts/release/identity.mjs` helper validates release IDs without
creating, sealing, uploading, or publishing a release record.

Orthic owns desktop installation. This package never supplies a DMG, NSIS,
Homebrew, WinGet, or Scoop installer.
