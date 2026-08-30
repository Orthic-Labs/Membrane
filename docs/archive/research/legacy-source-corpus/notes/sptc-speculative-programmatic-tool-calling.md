# sPTC — Speculative Programmatic Tool Calling

**Source:** Alex L. Zhang, blog post (2026) — https://alexzhang13.github.io/blog/2026/spec-ptc/
**Code:** https://github.com/alexzhang13/spec-ptc
**Related:** "The Bitter Lesson of Tool Calling" — arXiv:2608.06370
**Status:** research note (archived 2026-08-29); not an arXiv paper — a blog post with a reference
implementation. Non-normative provenance for CodeRight's execution-optimization track. This is a
**host/harness execution optimization**, not Membrane context-plane work: it never touches context
authority, admission, or provenance, and stays entirely inside the harness's tool-execution loop.

## Problem

Code-as-action harnesses (CodeAct, RLM-style) generate a REPL program whose tool calls (sub-LLM
invocations, search APIs) are high-latency and only start once generation finishes. Two wasted
overlaps: (1) time during token streaming before the cell completes; (2) sequentially written but
independent tool calls inside the cell that could run in parallel.

## Mechanism

Speculate and pre-launch tool calls from **partially generated** REPL code while tokens stream,
caching results as futures; when the completed cell actually executes, matching calls return
immediately from the speculation cache. Inspired by CPU speculative execution / speculative
decoding.

Shadowed execution: a deepcopy REPL fork parses and speculatively evaluates partial code —
`real_ns` keeps real tools; `shadow_ns` replaces tools with speculating stubs; during streaming
the shadow parses the growing cell, peeks variables, and queues speculations; real `exec` routes
through the promise cache.

Speculatable cases:

1. **Literals** — string/int tool inputs parsed directly (`llm_query("...")`).
2. **Safe dependencies** — inputs computed by pure functions run in the shadow REPL; dependent
   tools await the computed value.
3. **Peekable variables** — in-memory values from the working namespace available mid-stream.
4. **Blocked** — tools touching unsafe operations (`open()`, `write()`, marked non-pure) and their
   dependents are never speculated.

Implementation: promise/futures store keyed by tool inputs; idempotency tracking so one
speculation cannot serve multiple non-deterministic invocations; the shadow REPL never mutates
primary state; a REPL cell is the atomic unit, so incomplete/erroneous partial code cannot corrupt
real execution.

## Prior-art positioning

Conveyor (Xu et al., 2024) — partial execution during decoding; Speculative Interaction Agents
(Hooper et al., 2026) — formal tool speculation reducing TTFT; AsyncFC (Feng et al., 2026) —
future-based async wrappers. sPTC's claim: speculation inside a *program* (unknown runtime → more
overlap) is more useful than speculation on flat tool calls.

## Results

OOLONG (trec-coarse 132k) and OOLONG-Pairs (32k); 8×H100, vLLM, Qwen3-30B-A3B-Instruct-0527;
temp 0.7 and 0.0; 4 and 8 concurrent runs; 5 repetitions. Measured **1.0–1.2× wall-clock speedup**
on realistic RLM settings. Authors are explicit that exact gains depend on tool latency, token
volume, serving load, and trajectory shape. Overheads: parsing checks negligible; deepcopy cheap;
worst case is serving-engine congestion from concurrent speculated requests, mitigated by
controllable speculation aggressiveness and queuing.

## Limitations / future work

Handles common dependency patterns, not a full pseudo-compiler; currently {Python, bash, Bun} ×
{coding harness, RLM, game agent}; larger gains expected from JIT-style overlap of tool calls with
REPL execution as programs grow.

## Where this plugs in here

- **CodeRight execution runtime** (the "capability-code/sPTC" track): if/when CodeRight adopts
  code-as-action tool execution, sPTC-style speculation is a latency optimization for the
  tool-execution loop. Safety alignment is natural: speculation must respect the same effect
  boundaries as real execution — only pure/read-only tools are speculatable, and approval-gated or
  effectful tools are Case-4 blocked by construction.
- **Non-goal for Membrane:** the semantic advisor and context plane make at most 1–2 bounded model
  calls per trace and have no REPL; nothing here changes Membrane contracts.
- Honest expectation-setting: 1.0–1.2× is the measured realistic gain — worth having only once a
  programmatic tool-calling harness exists and its tool latency dominates.
