# Embedder bake-off — Cortex memory lane (2026-09)

**Decision: keep EmbeddingGemma-300M Q4.** Six candidates were scored against it on the Cortex
memory workload. Every one lost on graded relevance by a margin whose 95% bootstrap interval
excludes zero.

One real product bug was found while validating the benchmark against the shipped code path, and is
fixed in `engine/crates/cortex-core/src/embed.rs` (see *Production mismatches* below).

The harness and its frozen corpus are deliberately not checked in. This document is the evidence.

---

## 1. What was tested

| Arm | Repo | Dim | Pooling | Prompts | Licence |
|---|---|---|---|---|---|
| gemma (control) | onnx-community/embeddinggemma-300m-ONNX | 768 | `sentence_embedding` head | doc + query | Gemma terms |
| gte | Alibaba-NLP/gte-modernbert-base | 768 | CLS | none | Apache-2.0 |
| granite | sirasagi62/granite-embedding-english-r2-ONNX | 768 | `sentence_embedding` head | none | Apache-2.0 |
| voyage | onnx-community/voyage-4-nano-ONNX | 2048 | mean + head | doc + query | Apache-2.0 |
| voyage1024 / 512 / 256 | same weights, Matryoshka truncation + renormalize | 1024/512/256 | — | — | — |
| harrier270 | microsoft/harrier-oss-v1-270m | 640 | last token | Instruct query | see model card |
| jina v5 nano | jinaai/jina-embeddings-v5-text-nano | 768 | last token | `prompt_name` | CC-BY-NC-4.0 |

Jina was excluded from the deployment comparison: a non-commercial licence disqualifies it as a
default. Harrier has no Q4 ONNX export on the hub, so it was screened in bf16 on GPU rather than
through the production graph; its numbers are indicative, not production-path.

## 2. Method

- **Haystack:** 1,441 Cortex-shaped memory entries built from this machine's real gotchas, commit
  subjects and bodies, and memory files. Not a public IR benchmark — the workload Cortex actually
  serves.
- **Queries:** 137, graded, in three lanes — 65 plain, 41 low-vocabulary-overlap paraphrases, 31
  stale-versus-current adversarials.
- **Grades:** `3` direct answer, `2` clearly relevant, `1` weak background, `0` irrelevant, `-1`
  superseded and actively misleading.
- **Judgments re-pooled after every arm ran.** The top-12 union across all finalists was graded
  blind to which model surfaced each candidate, then merged with the hand-authored gold. That added
  608 non-zero grades including 7 new stale-answer traps — the original gold was materially
  incomplete, and scoring against it would have flattered whichever model authored it.
- **`-1` never enters nDCG as a negative gain.** The DCG term is `max(grade, 0)` and the ideal
  ranking is built only from grades `> 0`. Stale material is measured separately as the trap-escape
  rate: a query counts as an escape when a `-1` item outranks the first item graded `>= 2`.
- **Three retrieval layers, scored separately:** raw cosine, Membrane's hybrid reciprocal-rank
  fusion, and the Cortex-governed slate that drops superseded and expired rows.
- **Pipeline:** the exact Q4 ONNX graph, `tokenizer.json`, right truncation at 2048, pad to batch
  longest, L2 normalize, CPU execution provider, `intra_op_num_threads = 4`.

### Scope of the "production path" claim

This reproduces Membrane's embedding pipeline at the *declared* 2048-token sequence length, as a
controlled CPU benchmark at four intra-op threads. It is not a byte-exact replica of the shipped
runtime, which does not pin its thread count. Quality results are unaffected by that; latency
results are a controlled figure rather than a shipped one.

## 3. Quality — raw cosine, re-pooled judgments

nDCG@10 over 137 queries.

| lane | gemma | voyage | granite | harrier270 | gte |
|---|---|---|---|---|---|
| all | **0.821** | 0.743 | 0.712 | 0.698 | 0.683 |
| plain (65) | 0.851 | 0.817 | 0.800 | 0.800 | 0.801 |
| paraphrase (41) | **0.798** | 0.656 | 0.616 | 0.579 | 0.544 |
| stale-vs-current (31) | 0.789 | 0.701 | 0.653 | 0.643 | 0.621 |

Paired bootstrap against Gemma, nDCG, all queries — every arm, none containing zero:

| challenger | mean delta | 95% CI | P(better) |
|---|---|---|---|
| voyage (2048d) | -0.078 | -0.109 to -0.049 | 0.00 |
| voyage1024 | -0.088 | -0.117 to -0.057 | 0.00 |
| voyage512 | -0.093 | -0.125 to -0.062 | 0.00 |
| granite | -0.109 | -0.142 to -0.078 | 0.00 |
| harrier270 | -0.123 | -0.157 to -0.088 | 0.00 |
| voyage256 | -0.131 | -0.165 to -0.097 | 0.00 |
| gte | -0.138 | -0.179 to -0.098 | 0.00 |

**All arms tie on plain queries at roughly 0.80.** They separate only on paraphrases with no shared
vocabulary and on stale-versus-current questions. That is the lane where a dense model earns its
keep over lexical retrieval, and Gemma leads GTE there by 0.255 nDCG and Granite by 0.182.

Voyage's Matryoshka ladder does not rescue it: truncating from 2048 costs a further 0.010 at 1024d,
0.015 at 512d and 0.053 at 256d. A 512d index is 33% smaller than Gemma's 768d index, but buys no
quality.

## 4. Quality through the real stack

Hybrid fusion compresses the field: under Membrane's RRF all arms land between 0.624 and 0.681
nDCG, with recall@5 between 0.94 and 0.99. Adding the governed layer changes almost nothing except
traps — escapes fall to 0 of 15 for every arm.

Two conclusions. Supersession correctness is enforced by lifecycle state, not by vector similarity,
which is the architecturally healthy outcome. And fusion partially masks a weaker embedder, so raw
embedding quality still decides candidate quality as the corpus grows.

## 5. Cost

### Input distribution (memory entries, document prompt included)

| percentile | gemma tokens | gte / granite tokens |
|---|---|---|
| p50 | 18 | 12 |
| p95 | 414 | 434 |
| max | 1,298 | 1,342 |

### Peak RSS by batch size and entry length

Delta over the loaded session, one subprocess per measurement point.

| batch × length | gemma | gte | granite |
|---|---|---|---|
| 1 × p50 | 182 MB | 143 MB | 144 MB |
| 16 × p50 | 182 MB | 144 MB | 144 MB |
| 64 × p50 | 182 MB | 142 MB | 144 MB |
| 1 × p95 | 161 MB | 86 MB | 86 MB |
| 16 × p95 | 282 MB | 852 MB | 852 MB |
| 64 × p95 | 1,224 MB | 3,530 MB | 3,530 MB |
| 1 × max | 88 MB | 340 MB | 340 MB |
| 16 × max | 1,401 MB | 4,714 MB | 4,714 MB |
| 64 × max | **5,717 MB** | **19,466 MB** | **18,843 MB** |

Gemma is cheapest at every point above p50 and the gap widens with sequence length — sliding-window
attention against full attention in the ModernBERT and Granite encoders.

### Query latency, idle machine, CPU, batch 1, 4 threads (p50 / p95 ms)

| query | tokens | gemma | gte | granite |
|---|---|---|---|---|
| short | 7–14 | 138 / 178 | 22 / 25 | 22 / 24 |
| medium | 25–31 | 117 / 194 | 76 / 148 | 41 / 234 |
| long | 111–119 | 311 / 393 | 291 / 361 | 604 / 702 |

GTE and Granite are 5–6× faster than Gemma on short queries — the common interactive case — and
level on long ones. This is the one dimension where the challengers clearly win, and it did not
offset a 0.11–0.14 nDCG deficit.

### Artifact and index size

| model | Q4 graph | dim | bytes/vector | 50k memories |
|---|---|---|---|---|
| gemma | 198 MB | 768 | 3,072 | 154 MB |
| gte | 224 MB | 768 | 3,072 | 154 MB |
| granite | 224 MB | 768 | 3,072 | 154 MB |
| voyage | 243 MB | 2048 | 8,192 | 410 MB |

## 6. Production mismatches found while validating the path

### 6.1 Bundled Gemma truncated at 512 tokens, not 2048 — fixed

`EMBEDDING_MAX_SEQUENCE_TOKENS` is 2048 and the download path calls `.with_max_length(2048)`, but
the bundled branch called:

```rust
TextEmbedding::try_new_from_user_defined(user_model, InitOptionsUserDefined::new())
```

In fastembed 6.0.0 that builder defaults `max_length` to `DEFAULT_MAX_LENGTH`, which is **512**
(verified in the 6.0.0 source and in the cached 5.17.3 crate — same default, public field). So a
bundled install truncated at 512 while its own pipeline fingerprint declared 2048. The serious
consequence is not the score: reindex-skip decisions could treat vectors built under one truncation
policy as if they were built under the declared one.

Fixed by passing `InitOptionsUserDefined::new().with_max_length(EMBEDDING_MAX_SEQUENCE_TOKENS)`,
with a regression test that asserts the configured builder carries 2048 *and* that fastembed's own
default still differs, so the guard fails loudly if upstream changes. Rust verification in this
repository is CI-only, so that test is pushed rather than run locally.

Measured retrieval cost of the bug — memory haystack re-embedded at each cap and rescored:

| cap | truncated entries | nDCG@10 | R@5 |
|---|---|---|---|
| 256 | 9.2% | 0.814 | 0.964 |
| 384 | 4.3% | 0.816 | 0.964 |
| 512 (the bug) | 2.3% | 0.815 | 0.964 |
| 768 | 0.9% | 0.817 | 0.964 |
| 1024 | 0.1% | 0.818 | 0.964 |
| 2048 (declared) | 0.0% | 0.818 | 0.964 |

About 0.003 nDCG, confined to the long tail, because only 2.3% of entries exceed 512 tokens. (Rows
here are comparable to each other, not to the 0.821 headline, which uses a different batch shape.)

### 6.2 Thread count is not pinned — open

`InitOptionsUserDefined` leaves `intra_threads` at `None`, which fastembed documents as using every
available CPU core. No quality number depends on this, but the concurrency sweep argues the default
is wrong: at 4 concurrent callers, 10 threads was worse than 4 for every model (Gemma long-query
p50 1,342 ms against 2,754 ms). A resident daemon should cap this explicitly rather than let ONNX
Runtime claim every logical core. Left unchanged — it is a tuning decision deserving its own
measurement on target hardware, not a correctness fix.

### 6.3 Batch bound does not limit inference memory — open

`MAX_MEMORY_BATCH_ITEMS = 64` and `MAX_MEMORY_BATCH_CONTENT_CHARS = 256 KiB` (store.rs) place no
constraint on padded tokens, and the RSS matrix shows **64 × max is unsafe for every model tested**.
Cost scales roughly quadratically in padded sequence length and linearly in batch size, so the
control that works is a padded-token budget:

```rust
struct EmbeddingBatchPolicy {
    max_items: usize,           // 64, as today
    max_sequence_tokens: usize, // 2048
    max_padded_tokens: usize,   // items × longest tokenized item in batch
}
```

At ~16,384 padded tokens Gemma stays near 1.4 GB and the ModernBERT-family models near 4.7 GB. The
budget should be derived from an RSS target for supported machines rather than adopted as a
constant. Length-bucketing before packing keeps padding waste low so the budget binds on real work.

`FastEmbedder` already holds the ONNX session behind a `Mutex`, so calls do not run inference
concurrently; a bounded worker queue would be about batch formation and backpressure, not about
inventing serialization.

## 7. Limitations

- The code/docs chunk haystack was not completed. Membrane embeds memories only; that lane is a
  future workload.
- Harrier was screened in bf16 on GPU, not through a Q4 ONNX graph — no such export exists.
- Jina was not carried to the production path; its licence rules it out as a default.
- The memory corpus is synthesized from this machine's history rather than drawn from a live Cortex
  store, because no live store was available here.
