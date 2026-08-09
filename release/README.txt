Cortex — portable runtime bundle

This directory contains a self-contained Cortex installation:

  bin/cortex           POSIX launcher (macOS/Linux)
  bin/cortex.cmd       Windows command launcher
  bin/cortex.ps1       Windows PowerShell launcher
  bin/cortex-mcp       MCP server launcher
  lib/node             Bundled Node LTS runtime (no system Node required)
  app/package          Application files
  app/node_modules     Production dependencies
  app/grammars         Bundled Tree-sitter WASM grammars
  app/schemas          Public contract schemas
  LICENSE              License
  THIRD_PARTY_NOTICES  Third-party notices
  README.txt           This file

Run `bin/cortex --help` to verify the install. No system Node, npm, or
network is required.
