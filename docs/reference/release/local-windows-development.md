# Local Windows installer development

Adrian authorized this temporary internal-development route on 2026-09-07.
`.rightgit.json` retains GitHub CI & public-release policy. The explicit exception
is `.rightkit-local-development.json`: only `Orthic-Labs/Membrane`, native Windows,
& unsigned installer development. Remove that file to restore GitHub-only builds.

From primary Membrane checkout, run one command:

```powershell
pnpm run release:local:win:unsigned
```

It builds through RightKit, creates installer-bound `candidate.json` &
`sbom.json`, runs installed `internal-unsigned` qualification, then installs
that exact installer at stable `current`. Its final JSON names installer hash,
qualification evidence, installed root & version.

Build only:

```powershell
pnpm --dir apps/membrane-hub run release:build:win:unsigned
```

Existing packaging builds engine, daemon, tray & Hub through managed RightKit,
then packages those exact binaries into NSIS. RightKit owns persistent external
compiler/target caches, process containment & build receipts. No signing,
publication, Mac host, CI impersonation, or separate installer implementation is
part of this route. Never set `MEMBRANE_PUBLIC_CI_DIRECT_CARGO` locally.

Record source commit/tree identity, installer SHA-256 & measured build duration.
Install exact output, verify stable `current` bindings, then exercise Blueprint &
all explicit Membrane operations with Hub stopped before agent handoff. Hub starts
only for resident-process verification. Installer source is shared with GitHub;
Local build, focused local tests & installed acceptance gate internal delivery.
GitHub CI may validate pushed commits in parallel; do not wait for CI or dispatch
a duplicate hosted installer build for this internal loop. GitHub owns public
release qualification.

The first local build fills caches. Compare subsequent measured builds before
claiming a speed improvement. Updates currently use the next installer; Hub has
no wired update notification or install action.
