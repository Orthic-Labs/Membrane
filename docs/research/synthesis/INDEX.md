# Membrane Research Library

Local mirror of papers and articles relevant to Membrane's problem domain:
context engineering, durable agent memory, retrieval, and memory architectures.

Sources: [momo-research](https://github.com/momo-personal-assistant/momo-research)
paper list, plus canonical memory-architecture papers added for completeness.
Companion code mirrors were local, gitignored clones; see
[`../competitors/CORPUS-INDEX.md`](../competitors/CORPUS-INDEX.md) for upstream links.

## A — momo-research list (all items)

### Whitepapers & articles

| File | Title | Source | Membrane relevance |
|---|---|---|---|
| `papers/core/context_engineering/google-context-engineering-sessions-memory.pdf` | Context Engineering: Sessions & Memory | Google whitepaper (via google-agents-resources mirror) | Direct — sessions/working-memory vs long-term memory is Membrane's packet-vs-Persist split |
| `papers/articles/manus-context-engineering.md` | Context Engineering for AI Agents: Lessons from Building Manus | manus.im blog | Direct — KV-cache design, "mask don't remove", file-system-as-context; overlaps Membrane's Push/admission design |
| `papers/articles/chroma-context-rot.md` | Context Rot | research.trychroma.com | Direct — degradation of attention over long context; Membrane's freshness ranking + receipts-for-absence is the countermeasure |

### Papers

| File | Title | arXiv | Membrane relevance |
|---|---|---|---|
| `papers/core/agent_architectures/complexity-trap-observation-masking_2508.21433.pdf` | The Complexity Trap: Simple Observation Masking Is as Efficient as LLM Summarization for Agent Context Management | [2508.21433](https://arxiv.org/abs/2508.21433) | Direct, Push motion — supports deterministic compression (`runc`/`skel`/`compress`) over LLM summarization |
| `papers/core/context_memory/evo-memory-self-evolving-memory-benchmark_2511.20857.pdf` | Evo-Memory: Benchmarking LLM Agent Test-time Learning with Self-Evolving Memory | [2511.20857](https://arxiv.org/abs/2511.20857) | Persist motion — benchmark for evolving memory; informs a future curation/learning policy (ExpRAG, ReMem) |
| `papers/core/agent_architectures/multi-agent-evolving-orchestration_2505.19591.pdf` | Multi-Agent Collaboration via Evolving Orchestration | [2505.19591](https://arxiv.org/abs/2505.19591) | Adjacent — orchestration patterns, not memory proper |
| `papers/core/agent_architectures/codeact-executable-code-actions_2402.01030.pdf` | Executable Code Actions Elicit Better LLM Agents (CodeAct) | [2402.01030](https://arxiv.org/abs/2402.01030) | Adjacent — action representation |
| `papers/core/context_engineering/recursive-language-models_2512.24601.pdf` | Recursive Language Models | [2512.24601](https://arxiv.org/abs/2512.24601) | Adjacent — context-length recursion, tangential to context budgets |
| `papers/core/agent_architectures/darwin-godel-machine_2505.22954.pdf` | Darwin Gödel Machine: Open-Ended Evolution of Self-Improving Agents | [2505.22954](https://arxiv.org/abs/2505.22954) | Not memory — self-improving agents; harness territory |
| `papers/core/agent_architectures/automated-design-agentic-systems_2408.08435.pdf` | Automated Design of Agentic Systems | [2408.08435](https://arxiv.org/abs/2408.08435) | Not memory — meta agent design |
| `papers/core/agent_architectures/omni-epic_2405.15568.pdf` | OMNI-EPIC: Open-endedness via Models of Human Notions of Interestingness with Environments Programmed in Code | [2405.15568](https://arxiv.org/abs/2405.15568) | Not memory — open-ended environment generation |
| `papers/core/surveys/ontology-embedding-survey_2406.10964.pdf` | Ontology Embedding: A Survey of Methods, Applications and Resources | [2406.10964](https://arxiv.org/abs/2406.10964) | Moderate — structured knowledge beyond flat vectors; could inform Crypt storage organization |

## B — Added: canonical memory-architecture papers

| File | Title | arXiv | Why it's here |
|---|---|---|---|
| `papers/core/context_memory/2310.08560_MemGPT_Towards_LLMs_as_Operating_Systems.pdf` | MemGPT: Towards LLMs as Operating Systems | [2310.08560](https://arxiv.org/abs/2310.08560) | Foundational — OS-style paging between context and external storage; ancestor of Letta and of Membrane's admission model |
| `papers/core/context_memory/memorybank-long-term-memory_2305.10250.pdf` | MemoryBank: Enhancing Large Language Models with Long-Term Memory | [2305.10250](https://arxiv.org/abs/2305.10250) | Ebbinghaus forgetting curve for memory decay — the one policy Membrane lacks today |
| `papers/core/context_memory/titans-memorize-at-test-time_2501.00663.pdf` | Titans: Learning to Memorize at Test Time | [2501.00663](https://arxiv.org/abs/2501.00663) | Model-level memory; included because it's on the star list |
| `papers/core/context_memory/zep-temporal-knowledge-graph_2501.13956.pdf` | Zep: A Temporal Knowledge Graph Architecture for Agent Memory | [2501.13956](https://arxiv.org/abs/2501.13956) | Direct — temporal fact validity; maps to `membrane_temporal_fact` |
| `papers/core/context_memory/a-mem-agentic-memory_2502.12110.pdf` | A-MEM: Agentic Memory for LLM Agents | [2502.12110](https://arxiv.org/abs/2502.12110) | Zettelkasten-style dynamic memory linking; informs KnowledgeEmission structure |
| `papers/core/context_memory/memory-os-of-ai-agent_2506.06326.pdf` | Memory OS of AI Agent | [2506.06326](https://arxiv.org/abs/2506.06326) | Hierarchical memory with promotion/demotion — the paper behind the MemoryOS repo |
| `papers/core/context_memory/memos-memory-os-for-ai-system_2507.03724.pdf` | MemOS: A Memory OS for AI System | [2507.03724](https://arxiv.org/abs/2507.03724) | Memory-centric framework; plaintext/activation/parameter memory taxonomy |
| `papers/articles/anthropic-effective-context-engineering.md` | Effective Context Engineering for AI Agents | anthropic.com engineering blog | Direct — compaction, structured note-taking, sub-agent context isolation; parallels Membrane motions |

## C — Repos (surveyed upstream; clones were local and gitignored)

Systems previously assessed as Membrane-relevant (see chat analysis):
`memvid`, `comind`, `mengram`, `lettabot`, `claude-subconscious`, `honcho`,
`memory-lancedb-pro`, `hindsight`, `byterover-cli`, `mnemon`, `MemoryOS`,
`emulo`, `greplica`, plus `momo-research` (the source list) and
`BAI-LAB-MemoryOS` (code for the Memory OS paper above).

Not cloned (assessed not relevant): `momo-research`'s paper-only nature aside,
`Titans` demo repo (model-level) was skipped as code but its paper is above.

## Notes

- PDFs are current arXiv versions as of 2026-08-16; re-download from the arXiv
  link for newer revisions.
- Articles saved as markdown are text extractions of the live pages, not the
  original HTML; originals remain at the source URLs.
- Assessment framing: everything here is Membrane-domain (Persist/Pull/Push).
  None of it is Blueprint-domain — Blueprint is a repo truth graph, not a memory
  system. Memory-type systems rank below fresh Blueprint evidence in Membrane's
  admission policy.
