# Paper library manifest

48 papers, stored as extracted markdown rather than PDFs.
Original PDFs totalled 171 MB; the markdown is 4.6 MB (37x smaller).

PDFs are **not** committed — a repository is a poor place for 171 MB of third-party
papers, all of which are publicly downloadable. Each file's frontmatter carries
`source` (arXiv abstract page or publisher URL) and `source_pdf_sha256`, so the
exact original can be re-fetched and verified. Figures, equations and tables are
lost in extraction; fetch the PDF when they matter.

Regenerate a PDF's markdown with `pymupdf` `get_text`.

## adaptive retrieval

- `2502.01142_DeepRAG_MDP_Adaptive_Retrieval.md` — [2502.01142](https://arxiv.org/abs/2502.01142) — original 1,158,331 b, sha256 `8a2f320a4d671000…`
- `2504.12560_CDF_RAG_Causal_Dynamic_Feedback.md` — [2504.12560](https://arxiv.org/abs/2504.12560) — original 1,822,841 b, sha256 `4c42ff3173224d4c…`
- `2505.06569_MacRAG_Multi_Scale_Adaptive_Context_RAG.md` — [2505.06569](https://arxiv.org/abs/2505.06569) — original 7,672,771 b, sha256 `ebce54d228b140a1…`
- `2511.07328_Q_RAG_Value_Based_Multistep_Retrieval.md` — [2511.07328](https://arxiv.org/abs/2511.07328) — original 2,379,158 b, sha256 `5a201afbea342c6b…`

## agent architectures

- `automated-design-agentic-systems_2408.08435.md` — [2408.08435](https://arxiv.org/abs/2408.08435) — original 801,981 b, sha256 `32eb1c1a6888e35f…`
- `codeact-executable-code-actions_2402.01030.md` — [2402.01030](https://arxiv.org/abs/2402.01030) — original 4,345,392 b, sha256 `749a45e36cf89fc7…`
- `complexity-trap-observation-masking_2508.21433.md` — [2508.21433](https://arxiv.org/abs/2508.21433) — original 1,380,836 b, sha256 `e40b2fe085fe0a08…`
- `darwin-godel-machine_2505.22954.md` — [2505.22954](https://arxiv.org/abs/2505.22954) — original 3,825,399 b, sha256 `13ff4abe0c7ad4a7…`
- `multi-agent-evolving-orchestration_2505.19591.md` — [2505.19591](https://arxiv.org/abs/2505.19591) — original 7,407,808 b, sha256 `244c86ebd95a9fa7…`
- `omni-epic_2405.15568.md` — [2405.15568](https://arxiv.org/abs/2405.15568) — original 26,398,005 b, sha256 `ae55917c5173a03c…`

## context engineering

- `2512.05470_Everything_is_Context_Agentic_File_System.md` — [2512.05470](https://arxiv.org/abs/2512.05470) — original 181,999 b, sha256 `b2a8a219e70525d3…`
- `google-context-engineering-sessions-memory.md` — publisher URL in frontmatter — original 7,682,073 b, sha256 `c19044acc915ff2c…`
- `recursive-language-models_2512.24601.md` — [2512.24601](https://arxiv.org/abs/2512.24601) — original 9,942,446 b, sha256 `8567362c22768d9b…`

## context memory

- `2307.03172_Lost_in_the_Middle.md` — [2307.03172](https://arxiv.org/abs/2307.03172) — original 747,542 b, sha256 `653b29619eae2ae4…`
- `2309.06180_PagedAttention_vLLM.md` — [2309.06180](https://arxiv.org/abs/2309.06180) — original 1,459,631 b, sha256 `55b3b324d779a67c…`
- `2310.08560_MemGPT_Towards_LLMs_as_Operating_Systems.md` — [2310.08560](https://arxiv.org/abs/2310.08560) — original 663,708 b, sha256 `9f674bcff69c86f1…`
- `2412.15605_Dont_Do_RAG_Cache_Augmented_Generation.md` — [2412.15605](https://arxiv.org/abs/2412.15605) — original 137,492 b, sha256 `de83db84c31b017c…`
- `2502.11101_CacheFocus_Dynamic_Cache_Re_Positioning_for_RAG.md` — [2502.11101](https://arxiv.org/abs/2502.11101) — original 710,846 b, sha256 `9dbabbe0b81e14b4…`
- `2503.21760_MemInsight_Autonomous_Memory_Augmentation.md` — [2503.21760](https://arxiv.org/abs/2503.21760) — original 13,123,594 b, sha256 `e51a902a7770bdd5…`
- `a-mem-agentic-memory_2502.12110.md` — [2502.12110](https://arxiv.org/abs/2502.12110) — original 1,015,164 b, sha256 `fec32b521c4a1f79…`
- `evo-memory-self-evolving-memory-benchmark_2511.20857.md` — [2511.20857](https://arxiv.org/abs/2511.20857) — original 3,817,139 b, sha256 `ab99bebbc13f1815…`
- `memory-os-of-ai-agent_2506.06326.md` — [2506.06326](https://arxiv.org/abs/2506.06326) — original 849,565 b, sha256 `4b3cbeb6a94d6b5a…`
- `memorybank-long-term-memory_2305.10250.md` — [2305.10250](https://arxiv.org/abs/2305.10250) — original 516,536 b, sha256 `6c60f7f95a872de8…`
- `memos-memory-os-for-ai-system_2507.03724.md` — [2507.03724](https://arxiv.org/abs/2507.03724) — original 4,753,104 b, sha256 `9b9b71b61487ce9f…`
- `titans-memorize-at-test-time_2501.00663.md` — [2501.00663](https://arxiv.org/abs/2501.00663) — original 3,657,065 b, sha256 `a65e4a7d02784df1…`
- `zep-temporal-knowledge-graph_2501.13956.md` — [2501.13956](https://arxiv.org/abs/2501.13956) — original 148,771 b, sha256 `d26f7eb599540e8b…`

## evaluation

- `2408.08067_RAGChecker.md` — [2408.08067](https://arxiv.org/abs/2408.08067) — original 2,553,412 b, sha256 `1034bf8f0b909895…`
- `2506.04202_TracLLM_Context_Traceback_Long_Context_LLMs.md` — [2506.04202](https://arxiv.org/abs/2506.04202) — original 1,028,528 b, sha256 `9d12fec992ca3067…`
- `2603.16169_Open_Source_CRAG_Reproduction_and_Explainability.md` — [2603.16169](https://arxiv.org/abs/2603.16169) — original 723,070 b, sha256 `5ffdaa8776ee57e5…`

## graph structured retrieval

- `2408.08921_Graph_Retrieval_Augmented_Generation_Survey.md` — [2408.08921](https://arxiv.org/abs/2408.08921) — original 1,725,790 b, sha256 `345fa9030560d7f9…`
- `2501.00309_Retrieval_Augmented_Generation_with_Graphs_Survey.md` — [2501.00309](https://arxiv.org/abs/2501.00309) — original 8,896,579 b, sha256 `737b0b8fe0aa8459…`
- `2501.13958_Survey_GraphRAG_for_Customized_LLMs.md` — [2501.13958](https://arxiv.org/abs/2501.13958) — original 1,544,553 b, sha256 `f3875a5ede83c981…`
- `2507.03226_Efficient_KG_Construction_and_Retrieval_for_RAG.md` — [2507.03226](https://arxiv.org/abs/2507.03226) — original 740,019 b, sha256 `b0f6f9495e1ef0e7…`
- `2507.04127_BYOKG_RAG_Multi_Strategy_Graph_Retrieval.md` — [2507.04127](https://arxiv.org/abs/2507.04127) — original 514,845 b, sha256 `2594b02567a0f469…`
- `2507.16585_LLMxCPG_Code_Property_Graph_Guided_LLMs.md` — [2507.16585](https://arxiv.org/abs/2507.16585) — original 1,616,102 b, sha256 `6a8f2b1c94a64938…`

## rag core

- `2005.11401_Retrieval_Augmented_Generation_for_Knowledge_Intensive_NLP.md` — [2005.11401](https://arxiv.org/abs/2005.11401) — original 885,323 b, sha256 `23e3249e9a1e7541…`
- `2310.11511_Self_RAG.md` — [2310.11511](https://arxiv.org/abs/2310.11511) — original 1,405,127 b, sha256 `d9eaa1398abac0df…`
- `2401.15884_Corrective_RAG_CRAG.md` — [2401.15884](https://arxiv.org/abs/2401.15884) — original 667,756 b, sha256 `975aa1fd3c1b6031…`
- `2410.08821_DeepNote_Note_Centric_Deep_RAG.md` — [2410.08821](https://arxiv.org/abs/2410.08821) — original 2,253,500 b, sha256 `0f9190612a3b4e12…`

## repository context

- `2504.10046_GraphCodeAgent_Dual_Graph_Guided_LLM_Agent.md` — [2504.10046](https://arxiv.org/abs/2504.10046) — original 15,081,980 b, sha256 `54f5162f6c9ee18c…`
- `2509.25257_RANGER_Repository_Level_Agent_for_Graph_Enhanced_Retrieval.md` — [2509.25257](https://arxiv.org/abs/2509.25257) — original 3,299,196 b, sha256 `46d2f97b867c4e87…`
- `2510.17925_SpecAgent_Speculative_Retrieval_and_Forecasting_for_Code_Completion.md` — [2510.17925](https://arxiv.org/abs/2510.17925) — original 779,272 b, sha256 `1f8f1685e4cdb43b…`
- `2607.24882_Agent_Retrieval_Bench.md` — [2607.24882](https://arxiv.org/abs/2607.24882) — original 688,397 b, sha256 `8b4b6fecdf48207c…`

## security

- `2501.11759_Poison_RAG_Adversarial_Data_Poisoning.md` — [2501.11759](https://arxiv.org/abs/2501.11759) — original 883,897 b, sha256 `52a28d6400885424…`
- `2502.11127_G_Safeguard_Topology_Guided_Security_for_LLM_MAS.md` — [2502.11127](https://arxiv.org/abs/2502.11127) — original 10,134,288 b, sha256 `aec9c0dceaaa40bb…`

## surveys

- `2504.15909_Synergizing_RAG_and_Reasoning_Systematic_Review.md` — [2504.15909](https://arxiv.org/abs/2504.15909) — original 13,871,512 b, sha256 `0412310ea1e970f4…`
- `2506.00054_RAG_Comprehensive_Survey.md` — [2506.00054](https://arxiv.org/abs/2506.00054) — original 2,369,183 b, sha256 `a85e25a72c29602a…`
- `ontology-embedding-survey_2406.10964.md` — [2406.10964](https://arxiv.org/abs/2406.10964) — original 1,187,689 b, sha256 `3743f538668b1c04…`
