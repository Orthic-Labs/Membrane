# Portable install (zero prerequisites)

The portable runtime bundle contains its own Node LTS, all production
dependencies, the Tree-sitter WASM grammars, and the schemas. It needs no
system Node, no npm, and no network.

## Layout

```text
cortex/
  bin/cortex           POSIX launcher (macOS/Linux)
  bin/cortex.cmd       Windows command launcher
  bin/cortex.ps1       Windows PowerShell launcher
  bin/cortex-mcp       MCP server launcher
  lib/node             Bundled Node LTS
  app/package/         Application files
  app/node_modules/    Production dependencies
  app/grammars/        Bundled grammars
  app/schemas/         Public contract schemas
  LICENSE
  THIRD_PARTY_NOTICES
  README.txt
```

## Build

```sh
node scripts/release/stage-runtime.mjs --out /tmp/cortex-runtime
node scripts/release/build-runtime-bundle.mjs --out /tmp/cortex-runtime-archive
```

The release-candidate workflow matrix builds macOS arm64/x64, Windows x64,
and Linux x64/arm64 archives.

## Verify without system Node

```sh
env PATH=/usr/bin:/bin /tmp/cortex-runtime/cortex/bin/cortex --help
```

The launcher computes its own install root; it never uses global Node, global
npm, or the current working directory to locate app files.

## Uninstall

Delete the bundle directory. Portable installs do not enroll repositories or
install hooks; run `cortex init` explicitly for that.
