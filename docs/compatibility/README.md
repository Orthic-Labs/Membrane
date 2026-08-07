# Crypt compatibility facade

The legacy `crypt` crate and the legacy `crypt` / `crypt-service` binaries
remain available as a thin **compatibility facade** over the new `membrane`
crate. Existing scripts and downstream code keep working unchanged. New code
should target `membrane`.

## What is preserved

- The `crypt` crate re-exports the entire `membrane_runtime` public surface.
  Every existing public item keeps the same name, the same signature, and the
  same semantics. Downstream code that wrote `use crypt::foo;` keeps compiling
  and running.
- The `crypt` and `crypt-service` binaries keep accepting the same argv shape
  and producing the same exit codes.

## What is new

- On **first invocation** of any `crypt`-surface function in a process, the
  facade emits a single structured log line to stderr:

  ```text
  level=notice surface=crypt msg="crypt compatibility facade active; use membrane in new code"
  ```

- The wording is owned by `membrane_runtime::vocabulary`, which is the single
  source of truth for product-surface vocabulary. The membrane binary and the
  crypt facade read the same constants, so the wording cannot drift.

- The membrane binary's `cli` mode emits the matching product-surface notice
  on first invocation:

  ```text
  level=notice surface=membrane msg="this binary is membrane; the legacy crypt binary is a compatibility facade"
  ```

- The membrane crate now exposes membrane-native library entry points:
  `membrane::cli::run_cli`, `membrane::cli::run_cli_from`, and
  `membrane::serve::run_loopback_api`. Signatures are identical to the
  underlying `membrane_runtime::cli` and `membrane_runtime::serve` functions,
  so library callers can migrate one call site at a time.

## When to migrate from `crypt` to `membrane`

| Caller | Use |
|---|---|
| New code (CLI tools, MCP adapters, services) | `membrane` crate and `membrane` binary. |
| Existing scripts that launch `crypt` or `crypt-service` | Keep using them; the facade keeps them working. Migrate when convenient. |
| Existing Rust code that does `use crypt::foo;` | Either leave it (the re-export still works) or switch the path to `membrane_runtime::foo`. Either way, the function signatures are identical. |
| Existing operator runbooks | No change required. The migration notice is informational and emitted exactly once per process. |

## How the notice is measured

The notice is emitted exactly once per process per surface. The guard lives in
`membrane_runtime::vocabulary::emit_facade_notice_once`. Internally it uses
`AtomicBool::compare_exchange(false, true, SeqCst)` so the first caller wins
and every subsequent caller in the same process is silent. The crypt surface
and the membrane surface have independent guards so neither can suppress the
other.

## How to suppress the notice (operators)

The notice is informational. There is no built-in suppression switch in this
revision; it is meant to be visible during the migration window. Operators
who want a quiet stderr can redirect stderr (`2>/dev/null`) at the call site;
scripts that parse stdout are not affected because the notice goes to stderr.

## How to extend the vocabulary (developers)

All product-surface strings live in
`engine/crates/membrane-runtime/src/vocabulary.rs`. To add a new notice or a
new canonical string, add it there and re-export it from the
`membrane_runtime` lib root. Both the crypt facade and the membrane binary
must source their notices from this module so the wording stays single-sourced.

## File map

- `engine/crates/membrane-runtime/src/vocabulary.rs` — canonical product
  vocabulary, single source of truth.
- `engine/crates/crypt/src/lib.rs` — re-exports `membrane_runtime::*` and adds
  the `facade` module.
- `engine/crates/crypt/src/facade.rs` — migration-notice helpers for library
  callers (`ensure_migration_notice`, `migration_notice_text`).
- `engine/crates/crypt/src/main.rs` — CLI binary entry point, calls
  `ensure_migration_notice` first.
- `engine/crates/crypt/src/crypt_service_main.rs` — service binary entry
  point, calls `ensure_migration_notice` first.
- `engine/crates/membrane/src/cli.rs` — membrane-native `run_cli` and
  `run_cli_from`, with the product-surface notice.
- `engine/crates/membrane/src/serve.rs` — membrane-native `run_loopback_api`,
  with the product-surface notice.
- `engine/crates/membrane/src/modes.rs` — `dispatch_cli` stamps the
  product-surface notice on first invocation of the binary's `cli` mode.