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
| `loopback-api` | HTTP service bound to `127.0.0.1`. | Port range validated >=1024; runtime identity from the supervisor child. |
| `supervisor-child` | Resident child owned by Membrane Hub. | Hub-authenticated inherited stdio lifecycle frames; no lease file or argv. |

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
3. For `stdio-mcp` and `loopback-api`, the runtime owns its own transport framing. The binary
   is a thin dispatcher; it never buffers, splits, or reorders bytes.
4. For `supervisor-child`, Hub's inherited hello is validated against canonical root, executable
   digest, release generation, capability, & exact fence before startup.

## What it does not do

- It does not publish to Homebrew, npm, PyPI, or crates.io. Publishing is a separate
  decision owned by the release engineer and is documented in `MBR-901..912` once the
  Wave 3 release gate passes.
- It does not configure GitHub Actions or any CI runner. Every test in this crate runs through
  the user's local `rightkit cargo test`.
- It does not depend on `npm`, `npx`, `node`, or any other runtime. One binary, four modes.

## Verifying locally

```
rightkit cargo fmt --manifest-path engine/Cargo.toml --all -- --check
rightkit cargo test --manifest-path engine/Cargo.toml -p membrane
rightkit cargo build --manifest-path engine/Cargo.toml -p membrane --release
```

Both commands are part of the Book 1 deferred gate; they run at the end of Book 1, not on
every commit.
