# Historical MemRight Workspace Migration (R0.2, 2026-07-26)

> Historical migration record retained for lineage only. It documents retired
> MemRight paths & is not an active API, runtime, or configuration contract.

## Source

- **Source commit:** `e79c99842cf6e76d9b759a26d501917d01408d2a` (CodeRight repo, `main`)
- **Recovery worktree:** `D:/Claude/tools/.cache/memright-source-e79c9984`
- **Dirty state patch:** `D:/Claude/tools/.cache/memright-source-e79c9984.patch`
- **Manifest:** `D:/Claude/tools/.cache/memright-source-e79c9984.manifest.json`

## Imported paths and SHA-256 hashes (pre-import)

| Source path (from clean worktree) | Destination | SHA-256 (pre-import) |
|---|---|---|
| `engine/crates/memright/` | `crates/memright/` | See individual files below |
| `engine/crates/memright/src/main.rs` | `crates/memright/src/main.rs` | `C8C74E7D31CF76577BD21683047542E00AEC485F1EF05BB7FDD7CA8F43C2238D` |
| `engine/crates/memory/src/` | `crates/memright-core/src/` | See individual files below |
| `engine/crates/memory/src/lib.rs` | `crates/memright-core/src/lib.rs` | `B32B88F71994F7A7AF9174AF54F5F8E94527E79AEFC7A9E1A18D19F2156D18B9` |
| `engine/crates/config/src/okf.rs` | `crates/memright-format/src/okf.rs` (edited) | `2AF5FFA98DFCC9370AE1BA9B2A569F8A460C29A316C95E865594FB49495CFDEF` |

## Package renames

| Old package name | New package name | Notes |
|---|---|---|
| `coderight-memory` | `memright-core` | Path: `crates/memright-core/` |
| `coderight-config` (okf submodule only) | `memright-format` | Path: `crates/memright-format/` |
| `memright` | `memright` | Unchanged; dependencies updated |

## Import changes in Rust source

All `coderight_memory::` references → `memright_core::` in:
- `crates/memright/src/main.rs` (3 replacements)
- `crates/memright/src/serve.rs` (5 replacements)
- `crates/memright/src/store.rs` (11 replacements)
- `crates/memright/tests/db_first.rs` (7 replacements)
- `crates/memright/tests/embedder_probe.rs` (2 replacements)

All `coderight_config::` references → `memright_format::` in:
- `crates/memright/src/lib.rs` (1 replacement)
- `crates/memright/src/compress.rs` (1 replacement)

## memright-format extraction notes

The `memright-format` crate was created by extracting the OKF module from `coderight-config`:
- **Included:** All OKF types (`OkfBundle`, `OkfConcept`, `OkfLink`, etc.), `parse_bundle`, `emit_bundle`, `compress_prose`, and all supporting private functions.
- **Excluded:** `InstructionLoader::load_okf_bundle` method (depends on config-internal types `Frontmatter`, `InstructionFile`, `InstructionLoader`, `InstructionScope`).
- **Excluded:** `sanitize_instruction_body` function (depends on `coderight_policy::scan_prompt_injection`).
- These excluded items remain in CodeRight's config crate for CodeRight's own use.

## Recovery artifacts (local-only, never committed)

- `D:/Claude/tools/.cache/memright-source-e79c9984/` — detached worktree at source commit
- `D:/Claude/tools/.cache/memright-source-e79c9984-status.txt` — pre-migration dirty state
- `D:/Claude/tools/.cache/memright-source-e79c9984.patch` — binary diff of dirty state
- `D:/Claude/tools/.cache/memright-source-e79c9984.manifest.json` — provenance metadata
