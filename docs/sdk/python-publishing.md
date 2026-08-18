# Python SDK publishing policy

MBR-909 makes `membrane-client` (the Python package under `dist/packages/python/`)
ready to publish as a standalone PyPI distribution once its transport
contract is exercised through a live `tools/call` round trip (see "Known
gap" below) — and encodes, as machine-checked data rather than only prose,
that publishing this SDK must never mean publishing or bundling the
Membrane core application.

## What "publish the SDK, not the core app" means here

`dist/packages/python/package-boundary.v1.json` (validated against
`schemas/sdk-python-package-boundary.v1.schema.json` and cross-checked
against `pyproject.toml` and the on-disk tree by
`tests/sdk/python_package_boundary_test.py`) declares, as data:

- **`allowedTopLevelPackages: ["membrane_client"]`** — the sdist/wheel
  contains exactly one importable package: the transport-injected client
  and its content-free analysis helpers (`MembraneClient`, `ProtocolError`,
  `analyze_packet`, `analyze_receipt`).
- **`runtimeDependencies: []`** — zero third-party or first-party runtime
  dependencies. The client never vendors, downloads, or requires the
  Membrane daemon, another SDK, or a native extension to import.
- **`forbiddenPathPrefixes`** (`engine/`, `apps/`, `dist/packaging/`, `dist/release/`,
  `dist/install/`, `dist/npm/`, `dist/packages/typescript/`) — the repository locations of
  the Rust engine/daemon, the desktop app, installer/release tooling, the
  npm bootstrapper, and the TypeScript SDK. `pyproject.toml` must never
  reference any of them; the test asserts this by scanning the file's own
  text for each prefix.
- **`forbiddenFileExtensions`** — native binaries, installers, and archives
  (`.so`, `.pyd`, `.dylib`, `.dll`, `.exe`, `.app`, `.bin`, `.whl`, `.tar`,
  `.tar.gz`, `.zip`, `.dmg`, `.msi`, `.appimage`). The package tree is
  walked and asserted to contain none of them.
- **`corePublishing.publishesCoreApp: false`** and
  **`corePublishing.bundlesDaemonBinary: false`** — explicit, schema-typed
  (`const: false`) assertions, not just a comment, so a future edit that
  flips either to `true` fails the schema-shape test in
  `tests/sdk/python_package_boundary_test.py` instead of silently changing
  what "publish the SDK" means.

A user who needs the actual Membrane daemon installs it separately (see the
top-level installation docs); this package only points at it through the
transport function the caller injects — it never ships it.

## Packaging readiness this task adds

- `pyproject.toml` moves from placeholder `version = "0.0.0"` to `0.1.0`,
  matching the `0.1` semver baseline `docs/sdk/publishing.md` already
  declares for the Rust `membrane-protocol`/`membrane-provider-sdk` crates
  and the TypeScript client's `package.json` (`"version": "0.1.0"`), so the
  three SDKs stop implying different maturity levels for the same
  contracts.
- `src/membrane_client/py.typed` (PEP 561) plus
  `[tool.setuptools.package-data] membrane_client = ["py.typed"]` make the
  "typed client" claim in this package's description mechanically true for
  downstream type checkers, not just prose.
- `include-package-data = false` is set explicitly so a later, unrelated
  edit cannot silently pull stray files (docs, evidence, fixtures) into the
  sdist through setuptools' default auto-discovery.
- `readme` and `license` stay inside `dist/packages/python/` (`README.md`, and a
  `license.text` string naming the repository's actual
  `Orthic Labs Source Use License v1.0`, not a fabricated OSI license). No
  build backend is installed in this environment to prove a `readme`/
  `license` path that climbs out of the package directory (e.g.
  `../../docs/...`) actually resolves; keeping both references inside the
  package directory keeps the declared metadata testable today rather than
  asserting untested behavior.

## Release gate

`tests/sdk/python_package_boundary_test.py` is this task's release gate for
the boundary claim: it must pass before any future PyPI publish action, the
same way `engine/crates/membrane-provider-sdk/tests/downstream_fixture.rs`
gates the Rust crates in `docs/sdk/publishing.md`.
`tests/sdk/python_daemon_compatibility_test.py` is the compatibility gate
for this task's acceptance criterion ("Python SDK passes compatibility
tests against current and previous supported daemon"): see
`docs/sdk/python.md`'s "Daemon compatibility" section for what it proves
and what it does not yet prove.

Publishing to PyPI (or any index) stays a separately authorized release
action. This source change does not publish `membrane-client`, does not run
a build, and adds no automation that could publish it unattended.

## Known gap

Per `docs/sdk/http-transport.md` (MBR-308), the shared `McpServer::dispatch`
`tools/call` branch is currently a stub for every tool call regardless of
transport. No SDK — Python included — can complete a live operation round
trip today; `membrane-client`'s compatibility is proven only against the
canonical golden fixtures (`schemas/operations/operations/*.golden.json`) and the
schema-declared receipt-version window, not a running daemon. This task
does not paper over that: "publish-ready packaging" here means the
distribution boundary and metadata are correct and tested, not that a live
end-to-end round trip has been exercised.
