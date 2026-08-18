# Optional headless Docker/OCI image

`dist/packaging/oci/` is an optional evaluation-only headless container surface.
It is not desktop support or an Orthic installer replacement. Its committed
release record remains unavailable until independently produced artifacts and
receipts exist.

`scripts/release/oci/generate-oci-release.mjs` reads an externally supplied,
immutable release-generation record, exact image inputs, and real SBOM,
signature, trust, health, and secret-scan evidence. It hashes supplied files
and validates them through `scripts/release/verify-oci-release.mjs`; absent or
invalid evidence fails closed without changing the unavailable record.

`scripts/release/identity.mjs` supplies only deterministic source identity and
release-ID validation. It does not write release-generation evidence, build an
image, sign, push, or publish anything.
