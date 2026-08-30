# Research references — context plane, behavioral learning, attribution (2026-08-29)

**Status:** non-normative research notes. Research is evidence, not authority (Adapt canon §16).
Verify indexing stability of 2026-dated preprints before citing in durable material.

Extends the reference set in `docs/subsystems/MEMBRANE-CROSS-SUBSYSTEM-IMPROVEMENTS-AND-EVIDENCE-GATES.md` §16
(RRF, CRAG, Self-RAG, Lost in the Middle, LongLLMLingua, Braintrust/Langfuse/Phoenix) and the
comparators in Adapt canon §16 (Command Code Taste, CHIRON, HORKOS, Warp Skill Doctor).

## Semantic advisor / context plane (`docs/pending/MEMBRANE-SEMANTIC-ADVISOR-EXPERIMENTAL.md`)

- **ACE — Agentic Context Engineering** — arXiv:2510.04618 (2025). Context as an evolving playbook
  maintained by generator/reflector/curator roles making itemized incremental edits; names
  **brevity bias** (concise rewrites silently dropping domain heuristics) and **context collapse**
  (wholesale rewrites destroying accumulated guidance). Closest prior art to the
  proposer/deterministic-curator split; primary citation for the replace-not-append-but-never-
  wholesale-rewrite mutation constraint in harness evolution.
- **Sufficient Context** — arXiv:2411.06037 (ICLR 2025, Google). Sufficiency ("could these
  snippets alone answer it") is a distinct axis from relevance; stratifying by sufficiency lifts
  selective accuracy 2–10pp. Primary citation for the deterministic sufficiency check that gates
  corrective retrieval (pending doc §9, §13.1).
- **Adaptive-RAG** — arXiv:2403.14403 (2024). Route no-retrieval / single-step / multi-step by
  query complexity — supports the activation-gate posture (no LLM on every context request).
- **LLM-Independent Adaptive RAG** — arXiv:2505.04253 (2025). The retrieve/don't-retrieve gate can
  use lightweight non-LLM signals — supports keeping the trigger deterministic and outside the
  advisor.
- **RankGPT** — arXiv:2304.09542 (2023); **RankZephyr** — arXiv:2312.02724 (2023). Listwise LLM
  reranking as advisor; RankZephyr shows an open-weight distilled model can serve the
  `semantic_context_fast` / `local_only_semantic_context` profiles.
- **Context Rot** — Chroma Research report, 2025 (industry, not peer-reviewed). Monotonic
  degradation on 10k–500k-token tasks across 18 frontier models; **Classifier Context Rot** —
  arXiv:2605.12366 (2026) extends this to judge/evaluator components. Motivates bounded advisor
  input projections and the attention-budget stance beyond Lost in the Middle.

## Reduction / Push / budget (`MEMBRANE-PENDING-IMPLEMENTATION.md` §10–§11)

- **LLMLingua-2** — arXiv:2403.12968 (ACL Findings 2024). Task-agnostic token-classifier
  compression, 2–5x with fidelity — candidate technique for a measured reduction arm.
- **"Don't Break the Cache"** — arXiv:2601.06007 (2026). Prompt-cache-aware serving vs.
  long-horizon agent edits — supports the prompt-cache-compatibility constraint gate in harness
  evolution and cache-aware background review.

## Memory / Cortex admission and lifecycle

- **Zep (Graphiti)** — arXiv:2501.13956 (2025). Bi-temporal knowledge-graph memory with automatic
  fact invalidation and episode provenance — strongest reference for keeping provider authority
  and freshness distinct as first-class temporal mechanics.
- **Mem0** — arXiv:2504.19413 (2025). Production extraction+consolidation memory layer;
  comparator, not ontology.
- **Sleep-time Compute** — arXiv:2504.13171 (Letta/Berkeley 2025). Background consolidation off
  the user-facing hot path — maps to daemon-scheduled background review and warm-cache reuse.
- **Adaptive Memory Admission Control (A-MAC)** — arXiv:2603.04549 (2026). Memory admission as an
  explicit structured decision, not a generation byproduct — near-exact match to Cortex's
  admission-gate invariant.
- **MemGuard** — arXiv:2608.21867 (2026). Verifier signals persisted as lifecycle metadata that
  travels with the memory item — supports receipted mutable-state envelopes.

## Attribution and learning loop (Adapt canon §6.9; pending doc §2.5–§2.7)

- **Who&When** — arXiv:2505.00212 (ICML 2025 Spotlight). 127 annotated multi-agent failure logs;
  best methods reach 53.5% agent-level and 14.2% step-level attribution accuracy. Primary
  empirical support for refusing automatic root-cause claims and for the deterministic
  activation-evidence ladder over pure model judgment.
- **Seeing the Whole Elephant** — arXiv:2604.22708 (2026). Successor failure-attribution
  benchmark; track for held-out attribution evaluation.
- **Meta-Policy Reflexion** — arXiv:2509.03990 (2025). Reusable reflective memory with rule
  admissibility gating so lessons persist across tasks — closest match to recurrence-reduction as
  the north-star metric; a dedicated recurrence-rate benchmark does not yet exist in the
  literature.
- **Honest Lying: Memory Confabulation in Reflexive Agents** — arXiv:2605.29463 (2026). Agents
  write confident-but-false self-diagnoses into memory and keep reusing them — the exact failure
  mode the counterfactual-preventability gate and `already_correct` refusal exist to block.
- **GEPA** — arXiv:2507.19457 (2025, ICLR 2026). Reflective prompt evolution via Pareto merge of
  natural-language lessons; with **MIPROv2** (arXiv:2406.11695) and **PromptBreeder**
  (arXiv:2309.16797), the optimizer lineage for H7 variant generation — all subordinate to frozen
  eval sets and constraint gates here.
- **Dynamic Cheatsheet** — arXiv:2504.07952 (2025). Test-time curated strategy memory
  (generator+curator) — comparator for proposal-only curation.
- **SkillRL** — arXiv:2602.08234 (2026); **Workflow-to-Skill** — arXiv:2606.06893 (2026).
  Trajectory→skill induction pipelines — relevant to `skill_or_procedure` as a first-class
  intervention surface; induction output remains proposal-class here.
