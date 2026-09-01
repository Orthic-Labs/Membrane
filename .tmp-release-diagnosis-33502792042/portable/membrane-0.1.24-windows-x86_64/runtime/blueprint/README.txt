Blueprint — portable runtime bundle

This directory contains a self-contained Blueprint installation:

  bin/blueprint           macOS launcher
  bin/blueprint-mcp       MCP server launcher
  lib/node             Bundled Node LTS runtime (no system Node required)
  app/package          Application files
  app/package/node_modules Production dependencies
  app/grammars         Bundled Tree-sitter WASM grammars
  app/schemas          Public contract schemas
  LICENSE              License
  THIRD_PARTY_NOTICES  Third-party notices
  README.txt           This file

Run `bin/blueprint --help` to verify the install. No system Node, npm, or
network is required.
