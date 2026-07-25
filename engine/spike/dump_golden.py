"""Dump LLMLingua-2 compressed outputs (rate=0.5) for the parity test's golden file.

Run from the spike venv so llmlingua is importable:
  & spike/.venv/Scripts/python.exe spike/dump_golden.py
"""
from __future__ import annotations

import json
from pathlib import Path

SPIKE_ROOT = Path(__file__).resolve().parent
FIXTURES = SPIKE_ROOT / "fixtures"
OUT = SPIKE_ROOT / "golden_llmlingua_rate05.json"

from llmlingua import PromptCompressor  # noqa: E402

MODEL = "microsoft/llmlingua-2-bert-base-multilingual-cased-meetingbank"


def main() -> None:
    pc = PromptCompressor(model_name=MODEL, use_llmlingua2=True, device_map="cpu")
    out = {}
    for fx in sorted(FIXTURES.glob("*.txt")):
        text = fx.read_text(encoding="utf-8")
        result = pc.compress_prompt(
            text,
            rate=0.5,
            force_tokens=["\n", ".", "!", "?", ","],
        )
        out[fx.name] = {
            "compressed": result["compressed_prompt"],
            "origin_tokens": int(result["origin_tokens"]),
            "compressed_tokens": int(result["compressed_tokens"]),
        }
    OUT.write_text(json.dumps(out, indent=2, ensure_ascii=False), encoding="utf-8")
    for name, v in out.items():
        print(f"{name}: {v['origin_tokens']}→{v['compressed_tokens']} tokens, "
              f"{len(v['compressed'])} chars")


if __name__ == "__main__":
    main()
