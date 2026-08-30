# Membrane install contract

> See also: [installation/](installation/) for the manifest/IPC handshake contract and stable-roots reference.

Membrane Hub owns desktop activation, runtime supervision, release builds,
updates, & install cleanup. Canonical customer entry is branded R2 bootstrap at
`https://membrane.orthiclabs.com/install.ps1`; Infrastructure must route this
root URL to product-scoped R2 key `membrane/install.ps1` before it is live.
Immutable versioned bootstrap verifies a detached signed release manifest,
then downloads exact signed native archive from GitHub Releases. R2 stores
bootstrap only. Provisioned
RightRelease/R2 infrastructure owns publication; Membrane creates no uploader.

Membrane is public, so `@rightkit/git` GitHub Actions owns native compilation,
tests, SBOM/provenance, unsigned release candidates, Windows Azure OIDC signing,
and macOS Developer ID signing/notarization in protected `release` environment.
Protected Windows host consumes same-run signed Windows output for installed
qualification, sealing, and publication. Local compilation is diagnostic only
and cannot establish release proof.

Bootstrap stages a versioned user-local root, journals harness configuration,
atomically switches stable `current`, adds stable root to user `PATH`, runs
stable-path `membrane activate`, then verifies exact daemon generation and
harness bindings. Failure restores prior pointer, integration journal, and
healthy generation. Signed manifest is sole trust authority; `checksums.json`
is manifest-bound convenience output.

Production and development are isolated. Production executables, `PATH`,
startup, MCP, hooks, and client projections resolve only through user-local
stable `current` (`%LOCALAPPDATA%\Orthic Labs\Membrane\current` on Windows).
Repository, `dist`, `target`, `node_modules`, and version-specific paths are not
valid production bindings. `pnpm dev` uses origin `development`, a
checkout-scoped port, and separate config/data/cache/log roots under
`Membrane Dev/<checkout-id>`; it never reconciles global clients or mutates
installed state. `membrane activate` accepts only stable installed `current`
and reports installed origin, stable root, resolved version root, release
generation, and client state.

The installed Hub hosts the only Membrane runtime. Headless clients are
stateless and do not create a resident service. No external product manifest,
add-on handoff, or retired installer lane is
active. Membrane Hub is the only desktop runtime/build/release/activation
authority. Setup EXE, MSI, DMG, WinGet, and Homebrew are optional channels, not
primary distribution or activation prerequisites. Membrane is not installed as
a Windows Service.

Membrane Agent Plugins payload contains `plugin.json`, `mcp.json`, and only
public Membrane skills. Native thin client projections remain for unsupported
or client-specific surfaces. Agent Plugins never owns install, update, UI, or
activation.
OCI metadata remains evaluation-only until independently published evidence
exists.
