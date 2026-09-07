# Membrane CLI

The `membrane` binary is the single signed executable that services every Membrane entrypoint.
A clean user machine needs exactly one binary; everything below is a subcommand.

## Modes

```
membrane <mode> [flags]
```

| Mode | Purpose | Notes |
|---|---|---|
| `cli` | One-shot CLI subcommands (doctor, smoke, ingest, query, ...). | Forwards the tail to the runtime CLI. |
| `stdio-mcp` | JSON-RPC over stdio for MCP clients. | Line-delimited JSON, blocking until EOF. |

## Exit codes

| Code | Meaning |
|---:|---|
| 0 | Work completed. |
| 1 | Internal failure (SQLite / ONNX / panic). |
| 2 | User-visible error (bad argument, missing runtime, lifecycle rejection). |

`stderr` always starts with `membrane:` so scripts can grep on a stable prefix.

## Discovery order

1. The binary itself parses argv with `clap`. Unknown modes are rejected before any runtime call.
2. For `cli`, the runtime CLI parses the tail with the same `clap` schema the legacy `cortex`
   binary used, so existing scripts keep working.
3. `stdio-mcp` executes explicit operations through installed subsystem owners with Hub on or off.
   Its bounded session preserves diagnostic workspace state; it never auto-starts Hub or watchers.
4. Tray owns automatic resident processes. `hub_inactive` describes their inactivity;
   it never gates explicit memory, graph, context, Ledger, Adapt or Push operations.
   See [execution lifecycle boundary](../../architecture/execution-lifecycle-boundary.md).

## What it does not do

- It does not publish product artifacts. Publishing is a separate
  decision owned by the release engineer and is documented in `MBR-901..912` once the
  Wave 3 release gate passes.
- Native builds & tests run through managed GitHub CI for this public repository.
- Blueprint uses installer-bundled Node & provider assets; no developer checkout or system Node is required.

## Verifying locally

```
rightkit cargo fmt --manifest-path engine/Cargo.toml --all -- --check
rightkit cargo test --manifest-path engine/Cargo.toml -p membrane
rightkit cargo build --manifest-path engine/Cargo.toml -p membrane --release
```

Both commands are part of the Book 1 deferred gate; they run at the end of Book 1, not on
every commit.
