# LLMLingua-2 ONNX assets

This directory holds the **ONNX-exported LLMLingua-2 model** + its HF tokenizer
for the `compress` ONNX path (`cortex compress --rate 0.5` with the
`llmlingua-onnx` Cargo feature enabled).

The contents here are **gitignored** — populate locally before running the
ONNX-parity test (or any code that exercises the ONNX path):

- `model.onnx` — ~677 MB, the exported `microsoft/llmlingua-2-bert-base-multilingual-cased-meetingbank`
- `tokenizer.json` — ~2.8 MB, the HF `bert-base-multilingual-cased` tokenizer

The runtime needs `$ORT_DYLIB_PATH` pointing at a compatible `onnxruntime.dll`
(the one shipped with the Python `onnxruntime` wheel works fine).

## First-time fetch

From `membrane/engine`, in an isolated venv (the system Python's `transformers`
5.x has dropped an API the exporter still calls):

```bash
py -3.11 -m venv spike/.venv
spike\.venv\Scripts\python.exe -m pip install \
    "transformers>=4.55,<5" \
    "optimum[exporters]==1.27.0" \
    "torch==2.4.1+cpu" --index-url https://download.pytorch.org/whl/cpu \
    onnx onnxruntime

spike\.venv\Scripts\optimum-cli.exe export onnx \
    -m microsoft/llmlingua-2-bert-base-multilingual-cased-meetingbank \
    spike\export \
    --task token-classification --trust-remote-code

Copy-Item spike\export\model.onnx    crates\cortex\assets\llmlingua\model.onnx
Copy-Item spike\export\tokenizer.json crates\cortex\assets\llmlingua\tokenizer.json
```

(Why the pins: `transformers 5.x` removed `get_parameter_dtype`; `optimum 2.x`
gutted the `export onnx` CLI subcommand; `torch 2.4.x` is the last release
matching `optimum 1.27`'s `aten::scaled_dot_product_attention` symbolic
registration. See `spike/SPIKE-RESULT.md` for the full diagnosis.)

## Asset resolution at runtime

The Rust loader checks in order:

1. `$CORTEX_LLMLINGUA_MODEL` and `$CORTEX_LLMLINGUA_TOKENIZER` env vars
   (explicit override)
2. `crates/cortex/assets/llmlingua/model.onnx` and `tokenizer.json`
   relative to the crate manifest (the dev/test default)
3. Error with a clear pointer to this README

The integration test (`compress_matches_python_jaccard`) defaults to the
crate-manifest path; override with the env vars if your assets live elsewhere.

## Parity gate (spike result, 2026-07-01)

Python-on-Python parity harness at `spike/parity.py` on 5 prose fixtures:

| rate | word-Jaccard mean | re-tok Jaccard mean | gate (≥0.90) |
|------|-------------------|---------------------|--------------|
| 0.50 (default) | 0.952 | 0.946 | **PASS** |
| 0.33 (aggressive) | 0.866 | 0.889 | FAIL (borderline) |

The aggressive-rate gap is structural: llmlingua uses a **contextualized**
selection algorithm that beats naive top-K-by-probability at low keep rates.
Documented in `spike/SPIKE-RESULT.md` as a known limitation; revisit if real
workloads use rates < 0.4.
