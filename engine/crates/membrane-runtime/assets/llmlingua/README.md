# LLMLingua-2 Push assets

This directory holds optional ONNX-exported LLMLingua-2 model and HF tokenizer
for Membrane Push compression (`membrane cli push compress` with the
`llmlingua-onnx` Cargo feature enabled).

Contents are gitignored. Populate locally before running Push ONNX parity:

- `model.onnx` — ~677 MB, exported `microsoft/llmlingua-2-bert-base-multilingual-cased-meetingbank`
- `tokenizer.json` — ~2.8 MB, HF `bert-base-multilingual-cased` tokenizer

Runtime configuration:

1. `$MEMBRANE_PUSH_LLMLINGUA_MODEL` and `$MEMBRANE_PUSH_LLMLINGUA_TOKENIZER`
   provide explicit Push-owned paths.
2. `engine/crates/membrane-runtime/assets/llmlingua/model.onnx` and
   `tokenizer.json` are crate-local defaults.
3. Missing assets produce a typed Push degradation; heuristic reduction stays
   available.

`$ORT_DYLIB_PATH` must point to compatible ONNX Runtime when ONNX execution is
enabled.

## First-time fetch

From `membrane/engine`, use isolated exporter tooling:

```text
py -3.11 -m venv spike/.venv
spike\.venv\Scripts\python.exe -m pip install "transformers>=4.55,<5" "optimum[exporters]==1.27.0" "torch==2.4.1+cpu" --index-url https://download.pytorch.org/whl/cpu onnx onnxruntime
spike\.venv\Scripts\optimum-cli.exe export onnx -m microsoft/llmlingua-2-bert-base-multilingual-cased-meetingbank spike\export --task token-classification --trust-remote-code
```

Copy exported files into this directory. Version pins preserve exporter
compatibility; see `engine/spike/SPIKE-RESULT.md` for parity evidence.
