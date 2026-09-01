# Membrane install contract

> See also: [installation reference](README.md) for the manifest/IPC handshake contract and stable-roots reference.

Membrane native distribution owns desktop activation, updates, & install cleanup;
visible tray owns resident runtime supervision, while shared release tooling owns
release builds. Canonical customer entry is GitHub Pages bootstrap at
`https://membrane.orthiclabs.com/install.ps1`; DNS points this custom domain to
GitHub Pages. Bootstrap downloads release-owned installer logic from GitHub
Releases, which remains immutable payload authority. Membrane uses no package
manager, vendor distribution service, or product-local uploader.

Install from PowerShell:

`irm https://membrane.orthiclabs.com/install.ps1 | iex`

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

Visible native tray owns resident lifecycle. Its OS-coupled headless child daemon
hosts the only Membrane runtime; Hub dashboard opens on demand. Headless clients
are stateless and do not create a resident service. No external product manifest,
add-on handoff, or retired installer lane is
active. Tray-owned daemon is sole desktop runtime host; shared release tooling
owns build/publication, while Membrane install contract owns activation. GitHub
Pages plus GitHub Releases are sole public channel. Membrane is not installed as a
Windows Service.

Membrane Agent Plugins payload contains `plugin.json`, `mcp.json`, and only
public Membrane skills. Native thin client projections remain for unsupported
or client-specific surfaces. Agent Plugins never owns install, update, UI, or
activation.

On activation, native `membrane` registers its installed stable `current`
executable as `membrane stdio-mcp` for Claude Code and Codex through native
CLI registration, plus Cursor (`~/.cursor/mcp.json`), Windsurf
(`~/.codeium/windsurf/mcp_config.json`), and Antigravity
(`~/.gemini/config/mcp_config.json`) through atomic global-config merges. Missing
Claude or Codex clients are reported without
failing activation. `membrane deactivate` removes only bindings that still
match that exact stable executable; both commands support `--dry-run`.
OCI metadata remains evaluation-only until independently published evidence
exists.
