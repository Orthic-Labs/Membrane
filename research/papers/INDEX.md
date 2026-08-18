# Cortex Research Library

> Note: `../../repos/` is gitignored and not vendored in this repository —
> the companion code mirrors referenced below exist only in local research
> checkouts.

Local mirror of research relevant to Cortex's domain: evidence-backed code
graphs, symbol/code intelligence, change-impact analysis, and doc-vs-code
truth verification. Companion code mirrors live in [`../../repos/`](../../repos/).

Framing note: unlike Membrane's research library, everything here is about
*deriving truth from source*, not about agent memory. Memory-style systems
rank below this kind of fresh evidence in Membrane's admission policy.

## A — Code graphs & structural representation

| File | Title | Source | Cortex relevance |
|---|---|---|---|
| `papers/cpg-yamaguchi-2014.pdf` | Modeling and Discovering Vulnerabilities with Code Property Graphs | IEEE S&P 2014 ([author PDF](https://fabianyamaguchi.com/files/2014-ieeesp.pdf)) | Foundational — the Code Property Graph (AST+CFG+PDG fusion) is the ancestor of every code-graph design, incl. Joern; informs Neuron/Synapse edge typing |
| `papers/cpg-vulnerability-detection-cnn_2503.18175.pdf` | Enhancing Software Vulnerability Detection Using Code Property Graphs and CNNs | [arXiv:2503.18175](https://arxiv.org/abs/2503.18175) | Modern CPG usage; graph slicing over a unified representation |
| `papers/graphcoder-code-context-graph-retrieval_2406.07003.pdf` | GraphCoder: Enhancing Repository-Level Code Completion via Code Context Graph-based Retrieval | [arXiv:2406.07003](https://arxiv.org/abs/2406.07003) | Direct — tree-sitter-built code context graph + exact/semantic retrieval; closest published analogue to Cortex's graph retrieval |
| `papers/ranger-graph-enhanced-repo-agent_2509.25257.pdf` | RANGER: Repository-Level Agent for Graph-Enhanced Retrieval | [arXiv:2509.25257](https://arxiv.org/abs/2509.25257) | Direct — whole-repo knowledge graph (hierarchical + cross-file relations) consumed by an agent; validates graph-first orientation for agents |

## B — Doc-truth: code-comment & documentation consistency

| File | Title | Source | Cortex relevance |
|---|---|---|---|
| `papers/recite-stale-function-references_2608.03734.pdf` | We Must Have Missed This Comment: Detecting and Repairing Stale Function References in Linux Kernel Comments | [arXiv:2608.03734](https://arxiv.org/abs/2608.03734) | Direct — detecting/repairing stale symbol references in prose is exactly Cortex's doc-truth verification loop |
| `papers/comment-inconsistency-bert-longformer_2207.14444.pdf` | Code Comment Inconsistency Detection with BERT and Longformer | [arXiv:2207.14444](https://arxiv.org/abs/2207.14444) | NLI-framed comment/code consistency; baseline approach for claim-vs-source verdicts |
| `papers/comment-inconsistency-bug-introducing_2409.10781.pdf` | Investigating the Impact of Code Comment Inconsistency on Bug Introducing | [arXiv:2409.10781](https://arxiv.org/abs/2409.10781) | Empirical justification: stale docs aren't cosmetic, they cause bugs — the business case for doc-truth |

## C — Program analysis & impact (the graph's consumers)

| File | Title | Source | Cortex relevance |
|---|---|---|---|
| `papers/survey-llm-assisted-program-analysis_2502.18474.pdf` | A Contemporary Survey of LLM-Assisted Program Analysis | [arXiv:2502.18474](https://arxiv.org/abs/2502.18474) | Landscape of static/dynamic/hybrid analysis with LLMs; positions deterministic-first designs like Cortex |
| `papers/survey-246-static-code-analyzers_2602.18270.pdf` | A Survey of 246 Static Code Analyzers for Security | [arXiv:2602.18270](https://arxiv.org/abs/2602.18270) | Inventory of the analyzer ecosystem Cortex's precision ladder (COMPILER > AST > LEXICAL) competes with |
| `papers/survey-retrieval-augmented-codegen-repo-level_2510.04905.pdf` | Retrieval-Augmented Code Generation: A Survey with Focus on Repository-Level Approaches | [arXiv:2510.04905](https://arxiv.org/abs/2510.04905) | Survey of repo-level retrieval — the category Cortex's graph search/neighborhood APIs serve |
| `papers/repobench-repo-level-completion_2306.03091.pdf` | RepoBench: Benchmarking Repository-Level Code Auto-Completion Systems | [arXiv:2306.03091](https://arxiv.org/abs/2306.03091) | Benchmark separating retrieval quality from generation; useful template for Cortex retrieval benchmarks |

## D — Engineering design docs

| File | Title | Source | Cortex relevance |
|---|---|---|---|
| `papers/aider-repo-map-design.md` | Aider Repository Map Design | [aider.chat](https://aider.chat/docs/repomap.html) | PageRank-over-symbol-graph repo maps — the closest shipped analogue to Cortex orientation; worth comparing ranking choices |

## E — Repos added to `../../repos/` (2026-08)

Gap-fills against the existing clone set (shallow, `--depth 1`):

| Clone | Why |
|---|---|
| `sourcegraph__scip` | SCIP code-intelligence index format — the protocol tier of Cortex's precision ladder |
| `sourcegraph__lsif-go` | LSIF indexer; SCIP's predecessor, useful for format comparison |
| `joernio__joern` | The original Code Property Graph implementation; queryable code graph |
| `github__stack-graphs` | GitHub's stack graphs — file-local symbol navigation without full builds |
| `tree-sitter__tree-sitter-graph` | DSL for building graphs from tree-sitter parses — directly relevant to Cortex's mapping phase |
| `oracle__opengrok` | Classic cross-referencing code search engine |
| `CoatiSoftware__Sourcetrail` | Archived interactive code-graph explorer; reference UX for graph navigation |
| `oraios__serena` | LSP-based semantic code toolkit for agents — symbol-level retrieval/edit, a design comparator |
| `github__codeql` | CodeQL — code-as-database queries; the heavyweight code-graph query engine |

## Notes

- PDFs are current arXiv versions as of 2026-08-16; the CPG 2014 PDF is the
  author's copy (IEEE original is paywalled).
- The aider doc is a text extraction, not original HTML.
- Not cloned (already present): tree-sitter, ast-grep, semgrep, opengrep, oxc,
  dependency-cruiser, aider, CodeGraphContext, claude-context, repo-graph,
  code-index MCPs, cognee, llama_index.
