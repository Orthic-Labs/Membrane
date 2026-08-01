# MemRight Vector Backend Bake-off

One committed generator, config, dependency locks, harness, and receipt contract run independently on macOS and Windows. Arms are serialized per host; both hosts may run simultaneously.

## Prerequisites

- Python 3.9+
- Rust/Cargo 1.91+
- Git
- Network access for first Cargo build and exact sqlite-vec alpha source fetch

`protoc` is resolved from committed `protoc-bin-vendored` locks; no host install is used. Every backend has an isolated `Cargo.lock`, preventing stable/alpha sqlite symbols or Lance dependencies from sharing one binary.

## Windows: pull & smoke

From `D:\Claude`:

```powershell
git pull --ff-only origin main
git submodule update --init --recursive membrane
powershell.exe -NoProfile -ExecutionPolicy Bypass -File .\membrane\engine\vector-bakeoff\run.ps1 smoke
```

`run.ps1` starts Python hidden, waits, then prints captured output. Results land under `membrane\engine\vector-bakeoff\.results\smoke-windows-*`.

Full Round 1:

```powershell
powershell.exe -NoProfile -ExecutionPolicy Bypass -File .\membrane\engine\vector-bakeoff\run.ps1 round1
```

## macOS: smoke & Round 1

From `/Volumes/D/claude`:

```bash
membrane/engine/vector-bakeoff/run.sh smoke
membrane/engine/vector-bakeoff/run.sh round1
```

Results land under `membrane/engine/vector-bakeoff/.results/<mode>-<host>`.

## Resume & force

Runs resume by default only when executable-input digest and result SHA-256 both match. Changed fixture, config, source, lock, or launcher bytes invalidate saved runner results.

```bash
membrane/engine/vector-bakeoff/run.sh smoke --force
```

## Compare hosts

Copy or mount both complete result directories, then run:

```bash
membrane/engine/vector-bakeoff/run.sh compare \
  /path/to/mac/receipt.json \
  /path/to/windows/receipt.json
```

Comparison rejects input/config/manifest/matrix/fixture drift and requires identical candidate IDs for exact arms A, B, C1, C2, and D. ANN arms E, F, and research-only G preserve independent host metrics because index training may differ.

## Arm map

| Arm | Backend | Disposition |
|---|---|---|
| A | Current Rust dequantized/full sort | Exact control |
| B | Quantized resident/bounded top-N | Exact candidate |
| C1 | sqlite-vec 0.1.9 metadata-filtered | Exact |
| C2 | sqlite-vec 0.1.9 scope-partitioned | Exact |
| D | LanceDB 0.33 exact flat | Exact control |
| E | LanceDB IVF-HNSW-flat | ANN |
| F | LanceDB IVF-PQ + scalar prefilter indexes | ANN |
| G | sqlite-vec 0.1.10-alpha.4 DiskANN | Research-only |

Fixtures are synthetic and contain no memory content or production embeddings. Timings exclude fixture generation, process startup, serialization, and output writes; `buildNs` records each backend projection/index build.
