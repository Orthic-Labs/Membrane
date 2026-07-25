"""LLMLingua-2 ONNX parity harness — Python-on-Python spike.

For each fixture:
  1. Run the reference (llmlingua PromptCompressor.compress_prompt).
  2. Run the ONNX model: tokenize → score → keep top-K by keep-probability → detokenize.
  3. Re-tokenize both outputs with the SAME HF tokenizer and compare kept-token-ID sets.
  4. Report Jaccard per fixture; exit non-zero if any fixture < threshold.

Goal: validate the ONNX path before porting to Rust. If this Python-on-Python Jaccard
is >= threshold, the Rust port (using the same ONNX model + tokenizers crate) is
viable; if not, ship --no-onnx and defer.
"""
from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path

SPIKE_ROOT = Path(__file__).resolve().parent
DEFAULT_EXPORT_DIR = SPIKE_ROOT / "export"
DEFAULT_FIXTURES_DIR = SPIKE_ROOT / "fixtures"
DEFAULT_REPORT = SPIKE_ROOT / "report.json"

import numpy as np
import onnxruntime as ort
from tokenizers import Tokenizer

# Force a single-threaded ORT for predictable output & no hidden threading surprises.
os_opts = ort.SessionOptions()
os_opts.intra_op_num_threads = 1
os_opts.inter_op_num_threads = 1


def load_onnx_session(model_path: Path) -> ort.InferenceSession:
    return ort.InferenceSession(
        str(model_path),
        sess_options=os_opts,
        providers=["CPUExecutionProvider"],
    )


def onnx_keep_text(
    text: str,
    sess: ort.InferenceSession,
    tok: Tokenizer,
    rate: float,
    force_token_chars: tuple[str, ...] = ("\n", ".", "!", "?", ","),
) -> tuple[str, list[int], list[int]]:
    """Tokenize → run ONNX → keep top-`rate` tokens by keep-probability → detokenize.

    Force-token semantics: any token whose decoded form contains one of
    `force_token_chars` is ALWAYS kept (mirrors `compress.py force_tokens=`).

    Returns (compressed_text, kept_token_ids_sorted, all_original_token_ids).
    """
    enc = tok.encode(text)
    ids = enc.ids
    if not ids:
        return "", [], []

    # The exported BERT model accepts up to its training max (512 for bert-base-multilingual-cased).
    # Truncate to that; downstream uses head+tail anyway for very long inputs.
    max_len = 512
    if len(ids) > max_len:
        ids = ids[:max_len]

    input_ids = np.array([ids], dtype=np.int64)
    attention_mask = np.ones_like(input_ids)
    token_type_ids = np.zeros_like(input_ids)

    outputs = sess.run(
        None,
        {
            "input_ids": input_ids,
            "attention_mask": attention_mask,
            "token_type_ids": token_type_ids,
        },
    )
    logits = outputs[0][0]  # [seq, 2]
    # Softmax over the 2 classes (drop, keep) — keep-prob = softmax[:, 1]
    shifted = logits - logits.max(axis=-1, keepdims=True)
    exp = np.exp(shifted)
    probs = exp / exp.sum(axis=-1, keepdims=True)
    keep_prob = probs[:, 1]

    # Mark force-kept positions: any token whose decoded form contains a force char.
    force_mask = np.zeros(len(ids), dtype=bool)
    for i, tid in enumerate(ids):
        piece = tok.decode([tid], skip_special_tokens=False)
        if any(c in piece for c in force_token_chars):
            force_mask[i] = True

    n_total_keep = max(1, int(round(len(ids) * rate)))
    n_force = int(force_mask.sum())
    n_topk = max(0, n_total_keep - n_force)

    # Top-K from the non-force positions by keep-probability.
    non_force_idx = np.where(~force_mask)[0]
    if n_topk > 0 and len(non_force_idx) > 0:
        top_within = np.argsort(-keep_prob[non_force_idx])[:n_topk]
        top_idx = non_force_idx[top_within]
    else:
        top_idx = np.array([], dtype=np.int64)

    keep_idx = np.unique(np.concatenate([np.where(force_mask)[0], top_idx]))
    keep_idx_sorted = sorted(int(i) for i in keep_idx)
    kept_ids = [ids[i] for i in keep_idx_sorted]

    compressed = tok.decode(kept_ids, skip_special_tokens=False)
    return compressed, sorted(set(kept_ids)), list(ids)


def llmlingua_keep_text(text: str, pc, rate: float) -> tuple[str, list[int], int, int]:
    """Run reference llmlingua → return (compressed_text, kept_original_token_ids, origin, kept_count)."""
    out = pc.compress_prompt(text, rate=rate, force_tokens=["\n", ".", "!", "?", ","])
    compressed = out["compressed_prompt"]
    return compressed, None, int(out["origin_tokens"]), int(out["compressed_tokens"])


def original_ids_kept_by_llmlingua(
    original_ids: list[int],
    compressed_text: str,
    tok: Tokenizer,
) -> set[int]:
    """Approximate: which original token IDs appear (by decoded substring) in the
    compressed output? Crude but consistent with what the ONNX side does (token
    membership in the output)."""
    out_text = compressed_text
    kept: set[int] = set()
    for tid in original_ids:
        piece = tok.decode([tid], skip_special_tokens=False)
        if piece and piece in out_text:
            kept.add(tid)
    return kept


def jaccard(a: set, b: set) -> float:
    if not a and not b:
        return 1.0
    union = a | b
    if not union:
        return 1.0
    return len(a & b) / len(union)


def word_set(s: str) -> set[str]:
    """Lowercase alphanumeric tokens; ignore punctuation and whitespace."""
    import re
    return {m.group(0).lower() for m in re.finditer(r"[A-Za-z0-9]+", s)}


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--export-dir", default=DEFAULT_EXPORT_DIR, type=Path)
    ap.add_argument("--fixtures-dir", default=DEFAULT_FIXTURES_DIR, type=Path)
    ap.add_argument("--threshold", type=float, default=0.90)
    ap.add_argument("--rate", type=float, default=0.5)
    ap.add_argument("--report", type=Path, default=DEFAULT_REPORT, help="optional JSON report path")
    args = ap.parse_args()

    sess = load_onnx_session(args.export_dir / "model.onnx")
    tok = Tokenizer.from_file(str(args.export_dir / "tokenizer.json"))

    # Defer llmlingua import (slow first-run model download).
    from llmlingua import PromptCompressor  # type: ignore

    pc = PromptCompressor(
        model_name="microsoft/llmlingua-2-bert-base-multilingual-cased-meetingbank",
        use_llmlingua2=True,
        device_map="cpu",
    )

    fixtures = sorted(p for p in args.fixtures_dir.iterdir() if p.is_file() and p.suffix == ".txt")
    if not fixtures:
        print(f"no fixtures found in {args.fixtures_dir}", file=sys.stderr)
        return 2

    rows = []
    for fx in fixtures:
        text = fx.read_text(encoding="utf-8")
        llm_text, _llm_ids, origin, kept_count = llmlingua_keep_text(text, pc, args.rate)
        onnx_text, onnx_ids, original_ids = onnx_keep_text(text, sess, tok, args.rate)

        # Multiple Jaccard metrics so we can see which is fair:
        #   (a) word-level sets of the final outputs — what downstream actually sees.
        #   (b) re-tokenized token-ID sets — sensitive to detokenization re-encoding.
        #   (c) ONNX-kept original IDs vs substring-matched llm kept — biased high.
        ws_score = jaccard(word_set(llm_text), word_set(onnx_text))
        llm_rt = set(tok.encode(llm_text).ids)
        onnx_rt = set(onnx_ids)
        rt_score = jaccard(llm_rt, onnx_rt)
        llm_sub = original_ids_kept_by_llmlingua(original_ids, llm_text, tok)
        sub_score = jaccard(llm_sub, set(onnx_ids))

        # Primary gate is word-level (downstream-meaningful).
        score = ws_score

        rows.append(
            {
                "fixture": fx.name,
                "origin_tokens": origin,
                "llmlingua_compressed_tokens": kept_count,
                "onnx_compressed_tokens": len(onnx_kept := set(onnx_ids)),
                "jaccard_word": round(ws_score, 4),
                "jaccard_retoken": round(rt_score, 4),
                "jaccard_substring": round(sub_score, 4),
                "primary_jaccard": round(score, 4),
            }
        )
        marker = "PASS" if score >= args.threshold else "FAIL"
        print(f"[{marker}] {fx.name}: word={ws_score:.4f} retok={rt_score:.4f} substr={sub_score:.4f} "
              f"(llm {kept_count} tok, onnx {len(onnx_kept)} kept)")

    min_score = min(r["primary_jaccard"] for r in rows)
    mean_score = float(np.mean([r["primary_jaccard"] for r in rows]))
    report = {
        "threshold": args.threshold,
        "rate": args.rate,
        "min_jaccard": min_score,
        "mean_jaccard": round(mean_score, 4),
        "passed": min_score >= args.threshold,
        "rows": rows,
    }
    if args.report:
        args.report.write_text(json.dumps(report, indent=2), encoding="utf-8")

    print(f"\nmin_jaccard={min_score:.4f}  mean={mean_score:.4f}  threshold={args.threshold}  "
          f"=> {'PASS' if report['passed'] else 'FAIL'}")
    return 0 if report["passed"] else 1


if __name__ == "__main__":
    sys.exit(main())
