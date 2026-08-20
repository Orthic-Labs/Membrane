# Portable install (zero prerequisites)

The portable runtime bundle contains its own Node LTS, all production
dependencies, the Tree-sitter WASM grammars, and the schemas. It needs no
system Node, no npm, and no network.

## Layout

```text
blueprint/
  bin/blueprint           POSIX launcher (macOS/Linux)
  bin/blueprint.cmd       Windows command launcher
  bin/blueprint.ps1       Windows PowerShell launcher
  bin/blueprint-mcp       MCP server launcher
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
node scripts/release/stage-runtime.mjs --out /tmp/blueprint-runtime
node scripts/release/build-runtime-bundle.mjs --out /tmp/blueprint-runtime-archive
```

The release-candidate workflow matrix builds macOS arm64/x64, Windows x64,
and Linux x64/arm64 archives.

## Verify without system Node

```sh
env PATH=/usr/bin:/bin /tmp/blueprint-runtime/blueprint/bin/blueprint --help
```

The launcher computes its own install root; it never uses global Node, global
npm, or the current working directory to locate app files.

## Uninstall

Delete the bundle directory. Portable installs do not enroll repositories or
install hooks; run `blueprint init` explicitly for that.
