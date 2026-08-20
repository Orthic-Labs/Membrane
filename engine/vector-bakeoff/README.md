# Cortex Vector Backend Bake-off

One committed generator, config, dependency locks, harness, & receipt contract
run on macOS.

## Prerequisites

- Python 3.9+
- Rust/Cargo 1.91+
- Git
- Network access for first Cargo build and exact sqlite-vec alpha source fetch

`protoc` is resolved from committed `protoc-bin-vendored` locks; no host install is used. Every backend has an isolated `Cargo.lock`, preventing stable/alpha sqlite symbols or Lance dependencies from sharing one binary.

## macOS: smoke & Round 1

From `/Volumes/D/claude`:

```bash
cd membrane
engine/vector-bakeoff/run.sh smoke
engine/vector-bakeoff/run.sh round1
```

Results land under `engine/vector-bakeoff/.results/<mode>-mac`.

## Resume & force

Runs resume by default only when executable-input digest and result SHA-256 both match. Changed fixture, config, source, lock, or launcher bytes invalidate saved runner results.

```bash
engine/vector-bakeoff/run.sh smoke --force
```

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
