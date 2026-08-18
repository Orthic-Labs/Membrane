# Paper corpus

Reference papers behind Cortex's graph, retrieval, and incremental-build design.
Research input only — not product documentation and not implementation authority.

35 papers plus one design note. Filenames are `<arxiv-id>_Title.pdf`; the id is the
canonical identifier, so a paper appears exactly once.

## core

### Graph RAG

- [`2408.08921` — Graph Retrieval Augmented Generation Survey](core/graph_rag/2408.08921_Graph_Retrieval_Augmented_Generation_Survey.pdf)
- [`2501.00309` — Retrieval Augmented Generation with Graphs Survey](core/graph_rag/2501.00309_Retrieval_Augmented_Generation_with_Graphs_Survey.pdf)
- [`2501.13958` — Survey GraphRAG for Customized LLMs](core/graph_rag/2501.13958_Survey_GraphRAG_for_Customized_LLMs.pdf)
- [`2507.03226` — Efficient KG Construction and Retrieval for RAG](core/graph_rag/2507.03226_Efficient_KG_Construction_and_Retrieval_for_RAG.pdf)
- [`2507.04127` — BYOKG RAG Multi Strategy Graph Retrieval](core/graph_rag/2507.04127_BYOKG_RAG_Multi_Strategy_Graph_Retrieval.pdf)

### Incremental build & caching

- [`1503.07792` — Incremental Computation with Names](core/incremental_build/1503.07792_Incremental_Computation_with_Names.pdf)
- [`1610.00097` — Refinement Types for Precisely Named Cache Locations](core/incremental_build/1610.00097_Refinement_Types_for_Precisely_Named_Cache_Locations.pdf)
- [`1808.07826` — Fungi Typed Incremental Computation with Names](core/incremental_build/1808.07826_Fungi_Typed_Incremental_Computation_with_Names.pdf)

### Program analysis & code property graphs

- [`2014` — Yamaguchi Code Property Graphs IEEE SP](core/program_analysis/2014_Yamaguchi_Code_Property_Graphs_IEEE_SP.pdf)
- [`2207.14444` — Comment Code Inconsistency BERT Longformer](core/program_analysis/2207.14444_Comment_Code_Inconsistency_BERT_Longformer.pdf)
- [`2409.10781` — Comment Inconsistency Bug Introducing Changes](core/program_analysis/2409.10781_Comment_Inconsistency_Bug_Introducing_Changes.pdf)
- [`2412.10164` — KEEP IT SIMPLE ANGLE Code Graph Vulnerability Detection](core/program_analysis/2412.10164_KEEP_IT_SIMPLE_ANGLE_Code_Graph_Vulnerability_Detection.pdf)
- [`2503.18175` — CPG and CNN Vulnerability Detection](core/program_analysis/2503.18175_CPG_and_CNN_Vulnerability_Detection.pdf)
- [`2504.16877` — Context Enhanced Vulnerability Detection with LLM](core/program_analysis/2504.16877_Context_Enhanced_Vulnerability_Detection_with_LLM.pdf)
- [`2507.16585` — LLMxCPG Code Property Graph Guided LLMs](core/program_analysis/2507.16585_LLMxCPG_Code_Property_Graph_Guided_LLMs.pdf)
- [`2608.03734` — ReCITE Stale Function References](core/program_analysis/2608.03734_ReCITE_Stale_Function_References.pdf)

### Repository-level retrieval & completion

- [`2306.03091` — RepoBench Repository Level Completion](core/repository_retrieval/2306.03091_RepoBench_Repository_Level_Completion.pdf)
- [`2406.07003` — GraphCoder Code Context Graph Retrieval](core/repository_retrieval/2406.07003_GraphCoder_Code_Context_Graph_Retrieval.pdf)
- [`2504.10046` — GraphCodeAgent Dual Graph Guided LLM Agent](core/repository_retrieval/2504.10046_GraphCodeAgent_Dual_Graph_Guided_LLM_Agent.pdf)
- [`2509.25257` — RANGER Repository Level Agent for Graph Enhanced Retrieval](core/repository_retrieval/2509.25257_RANGER_Repository_Level_Agent_for_Graph_Enhanced_Retrieval.pdf)
- [`2510.17925` — SpecAgent Speculative Retrieval and Forecasting for Code Completion](core/repository_retrieval/2510.17925_SpecAgent_Speculative_Retrieval_and_Forecasting_for_Code_Completion.pdf)
- [`2607.24882` — Agent Retrieval Bench](core/repository_retrieval/2607.24882_Agent_Retrieval_Bench.pdf)
- [aider repo map design](core/repository_retrieval/aider-repo-map-design.md)

## overlap

### Evaluation & falsification

- [`2408.08067` — RAGChecker](overlap/evaluation_falsification/2408.08067_RAGChecker.pdf)
- [`2506.04202` — TracLLM Context Traceback Long Context LLMs](overlap/evaluation_falsification/2506.04202_TracLLM_Context_Traceback_Long_Context_LLMs.pdf)

### Retrieval planning & adaptive retrieval

- [`2310.11511` — Self RAG](overlap/retrieval_planning/2310.11511_Self_RAG.pdf)
- [`2401.15884` — Corrective RAG CRAG](overlap/retrieval_planning/2401.15884_Corrective_RAG_CRAG.pdf)
- [`2502.01142` — DeepRAG MDP Adaptive Retrieval](overlap/retrieval_planning/2502.01142_DeepRAG_MDP_Adaptive_Retrieval.pdf)
- [`2504.12560` — CDF RAG Causal Dynamic Feedback](overlap/retrieval_planning/2504.12560_CDF_RAG_Causal_Dynamic_Feedback.pdf)
- [`2511.07328` — Q RAG Value Based Multistep Retrieval](overlap/retrieval_planning/2511.07328_Q_RAG_Value_Based_Multistep_Retrieval.pdf)

### Security & adversarial retrieval

- [`2501.11759` — Poison RAG Adversarial Data Poisoning](overlap/security_adversarial/2501.11759_Poison_RAG_Adversarial_Data_Poisoning.pdf)
- [`2502.11127` — G Safeguard Topology Guided Security for LLM MAS](overlap/security_adversarial/2502.11127_G_Safeguard_Topology_Guided_Security_for_LLM_MAS.pdf)

### Surveys

- [`2502.18474` — Survey LLM Assisted Program Analysis](overlap/surveys_overlap/2502.18474_Survey_LLM_Assisted_Program_Analysis.pdf)
- [`2504.15909` — Synergizing RAG and Reasoning Systematic Review](overlap/surveys_overlap/2504.15909_Synergizing_RAG_and_Reasoning_Systematic_Review.pdf)
- [`2510.04905` — Survey Retrieval Augmented CodeGen Repo Level](overlap/surveys_overlap/2510.04905_Survey_Retrieval_Augmented_CodeGen_Repo_Level.pdf)
- [`2602.18270` — Survey 246 Static Code Analyzers](overlap/surveys_overlap/2602.18270_Survey_246_Static_Code_Analyzers.pdf)
