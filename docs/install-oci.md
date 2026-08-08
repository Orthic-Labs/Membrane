# Optional headless Docker/OCI image (MBR-910)

This document describes the **optional, headless** container image under
`packaging/oci/`. It exists for manually exercised Linux/server evaluation
of Membrane's headless daemon/CLI surface only. It is not, and does not
imply, desktop-app support on Linux.

For the full inventory of source contracts (macOS, Windows, npm, Homebrew,
this OCI image), see `docs/install.md`. This file is the OCI-specific
companion required by MBR-910's allowlist and goes into more depth than that
shared overview.

## What this is, and is not

- **Is**: a rootless, digest-pinned container definition an operator can
  manually build (`podman build` / `docker build`) and run on their own
  Linux/server host, once a real release backs it, to exercise Membrane's
  headless daemon/CLI without a desktop environment.
- **Is not**: the way most users should install Membrane. macOS and Windows
  desktop installers remain the supported default. This image is never
  advertised as a replacement for them.
- **Is not**: a CI/automation artifact. Book-Mode's MBR-910 owner override
  states this explicitly: "CI integration and workflow configuration are
  out of scope and forbidden." Nothing in `packaging/oci/`,
  `scripts/release/oci/`, or `tests/oci/` invokes, installs, or configures
  a CI runner, workflow, or hosted/self-hosted automation service. Every
  command here is meant to be typed by a human on a machine they control.

## Support tier: currently none

`docs/support-matrix.json` (generated from real conformance receipts, see
`docs/support-matrix.md`) does not carry a Linux/OCI row today — the
generator's inputs (`scripts/qualification/run.mjs` +
`scripts/qualification/verify-mbr801-evidence.mjs`) only qualify macOS and
Windows installed-path clients, and every row currently reads `tier:
"unavailable"` because no release evidence has been published yet
(`generatedFrom.releaseGeneration: null`). This document, the Containerfile,
and `packaging/oci/release.v1.json` must never claim a support tier the
matrix does not back. Until a Linux/OCI row exists in that generated matrix
with a real receipt, this image is **evaluation-only, unsupported**: no SLA,
no compatibility guarantee across versions, no signed support commitment.

## Current state: unavailable, by design

`packaging/oci/release.v1.json` (`schema:
"orthic.membrane.oci-release.v1"`) currently has `state: "unavailable"` and
deliberately invalid placeholders — an all-zero digest base, a
`registry.invalid/` image, an `UNAVAILABLE` tag, all-zero identity hashes,
and every evidence slot (`sbom`, `ed25519`, `cosign`, `rootlessHealth`,
`secretScan`) set to `null`. **Do not build, pull, or publish this image
today.** `tests/oci/oci-contract.test.mjs` enforces that the file cannot
silently drift out of this fail-closed state, and
`scripts/release/verify-oci-release.mjs` is the schema this document and
every OCI script defer to.

## How this binds to a real release

MBR-903 (`scripts/release/pipeline/multi-platform-release.mjs`) is the one
place Membrane's release identity — `tag`, `commit`, `release_generation` —
gets computed and immutably recorded, at
`evidence/releases/<releaseId>/release-generation.json`. This task adds
`scripts/release/oci/generate-oci-release.mjs`, which is the **only**
supported way to move `packaging/oci/release.v1.json` from `unavailable` to
`ready`. It:

1. Reads that release-generation record and refuses to proceed unless it
   already exists, has the exact MBR-903 schema, stays `publish: false`, and
   its own `releaseId` recomputes correctly from its `app`/`version`/`commit`
   (catching a hand-edited or cross-pasted record).
2. Copies `identity.tag`, `identity.commit`, and `identity.release_generation`
   **verbatim** from that record — never re-typed or re-derived by hand.
3. Requires the exact digest-pinned base image the operator built with
   (`--base`) to match `packaging/oci/Containerfile`'s `FROM` line, catching
   drift between what the contract says and what was actually built.
4. Requires a real, already-built Linux `membrane` binary (`--binary`) —
   the same one `COPY`'d into the image — and computes its sha256 itself
   (`identity.artifact_sha256`); this is never a hand-typed hash.
5. Requires five real, already-produced evidence files (`--sbom --ed25519
   --cosign --rootless-health --secret-scan`), each hashed from the actual
   file on disk.
6. Hands the assembled candidate to `verifyOciRelease` (unmodified from the
   existing MBR-910 source contract) as the single source of truth for what
   a valid `ready` release looks like, before writing anything.
7. Only if every input is present and the candidate validates does it write
   `packaging/oci/release.v1.json`; any missing or invalid input fails
   closed and leaves the file exactly as it was.
8. Once a `ready` release is recorded, writing a genuinely different
   candidate for the same file is refused rather than silently overwritten —
   mirroring MBR-903's own `writeImmutableReleaseGeneration` idempotency
   rule.

```text
node scripts/release/oci/generate-oci-release.mjs \
  --release-generation evidence/releases/<releaseId>/release-generation.json \
  --image ghcr.io/orthic/membrane@sha256:<real image manifest digest> \
  --base cgr.dev/chainguard/static@sha256:<real base image digest, matching Containerfile FROM> \
  --binary <path to the real Linux membrane binary> \
  --sbom <path to a real SBOM> \
  --ed25519 <path to a real Ed25519 signature> \
  --cosign <path to a real cosign receipt> \
  --rootless-health <path to a real rootless-run health receipt> \
  --secret-scan <path to a real secret-scan report> \
  --write
```

This command never builds, signs, pushes, or runs anything itself — every
path it takes must already exist on disk, produced by the operator's own
manual build/sign/scan steps.

## Declared gaps (not fabricated, not silently filled)

- **No Linux target in the release pipeline.** MBR-903's four logical
  release targets (`release/contracts/platforms.v1.json`) are
  `mac-arm64`, `mac-x64`, `windows-x64`, `windows-arm64` — there is no
  `linux` entry, and this task does not add one (that file is outside
  MBR-910's allowlist). The OCI image is bound to the *same source
  identity* (`commit` / `tree` / `release_generation`) as the desktop
  builds via `release-generation.json`, but is built from source directly
  by the operator; it is not a RightKit-sealed platform target.
- **`membrane cli doctor paths` is a diagnostic, not an app-specific health
  probe.** It proves the binary runs, resolves its roots, and exits 0 —
  real, headless, and already covered by `engine/crates/membrane`'s own
  tests (`dispatch.rs`, `modes.rs`). It does not deep-probe daemon
  liveness; a more specific probe is future work, not invented here.
- **SBOM/signature/cosign/secret-scan tooling is not installed or invoked
  by this task.** Nothing here runs `syft`, `cosign`, `trivy`, or similar —
  doing so would require installing a dependency and running a build tool,
  both outside this task's hard rules. `generate-oci-release.mjs` only
  validates and hashes evidence *files an operator already produced*.
- **No image has ever been built, signed, or pushed.** `state:
  "unavailable"` remains accurate as of this task; nothing in this change
  set claims otherwise.

## Runtime security posture

- **Non-root by default.** `USER 65532:65532` in `packaging/oci/Containerfile`
  (a "nobody"-style high UID/GID with no matching `/etc/passwd` entry, the
  same convention Chainguard/distroless base images use) — the process
  never runs as UID 0, satisfying "rootless" for the container's own user
  namespace. Operators wanting full rootless *engine* semantics
  (`podman run --userns=keep-id` or an equivalent Docker rootless daemon)
  layer that on top; this image does not fight it.
- **No embedded credentials.** `tests/oci/oci-contract.test.mjs` asserts the
  Containerfile never contains a `SECRET=`, `TOKEN=`, `PASSWORD=`, or
  `API_KEY=` assignment. Any real secret Membrane needs at runtime must be
  supplied by the operator's own container/orchestration secret mechanism,
  never baked into the image.
- **No network service exposed by default.** There is no `EXPOSE` in the
  Containerfile (enforced by
  `tests/oci/oci-contract.test.mjs`'s "exposes no network port by default"
  test), matching Membrane's local-first/loopback-bound guarantee
  (`docs/agent-rules.md`: "Keep data local, loopback-bound, and
  repository-confined"). The image runs the CLI/daemon exactly as it runs
  on macOS/Windows — bound to loopback, not to `0.0.0.0` — and an operator
  who wants MBR-306's authenticated loopback HTTP transport reachable from
  outside the container must explicitly publish a port and configure that
  transport themselves; this image does not do it for them.
- **Immutable root filesystem is the operator's choice, not assumed here.**
  The only writable path this image expects is the declared
  `/var/lib/membrane` volume (`MEMBRANE_ROOT`); everything else is copied
  in at build time.
