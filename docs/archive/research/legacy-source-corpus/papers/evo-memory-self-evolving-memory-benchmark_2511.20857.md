---
title: evo-memory-self-evolving-memory-benchmark
topic: context_memory
arxiv: 2511.20857
source: https://arxiv.org/abs/2511.20857
pages: 27
source_pdf_sha256: ab99bebbc13f18150f888c43e27cde903751b3e5519de0104fe183af3bd46618
converted_by: pymupdf get_text
---
2026-5-19
Evo-Memory: Benchmarking LLM Agent
Test-time Learning with Self-Evolving Memory
Tianxin Wei†,1, Noveen Sachdeva2, Benjamin Coleman2, Zhankui He2, Yuanchen Bei1, Xuying Ning1, Mengting
Ai1, Yunzhe Li†,1, Jingrui He1, Ed H. Chi2, Chi Wang2, Shuo Chen2, Fernando Pereira2, Wang-Cheng Kang2 and
Derek Zhiyuan Cheng2
†Work done while at Google DeepMind, 1University of Illinois Urbana-Champaign, 2Google DeepMind
Statefulness is essential for large language model (LLM) agents to perform long-term planning and
problem-solving. This makes memory a critical component, yet its management and evolution remain
largely underexplored. Existing evaluations mostly focus on static conversational settings, where memory
is passively retrieved from dialogue to answer queries, overlooking the dynamic ability to accumulate
and reuse experience across evolving task streams. In real-world environments such as interactive
problem assistants or embodied agents, LLMs are required to handle continuous task streams, yet often
fail to learn from accumulated interactions, losing valuable contextual insights, a limitation that calls for
test-time evolution, where LLMs retrieve, integrate, and update memory continuously during deployment.
To bridge this gap, we introduce Evo-Memory, a comprehensive streaming benchmark and framework
for evaluating self-evolving memory in LLM agents. Evo-Memory structures datasets into sequential task
streams, requiring LLMs to search, adapt, and evolve memory after each interaction. We unify and
implement over ten representative memory modules and evaluate them across 10 diverse multi-turn
goal-oriented and single-turn reasoning and QA datasets. To better benchmark experience reuse, we
provide a baseline method, ExpRAG, for retrieving and utilizing prior experience, and further propose
ReMem, an action–think–memory refine pipeline that tightly integrates reasoning, task actions, and
memory updates to achieve continual improvement.
Keywords: LLMs, Agentic Memory, Test-time Learning, Self-evolving Agents, Lifelong Intelligence
1. Introduction
Large Language Models (LLMs) have rapidly evolved from simple chatbots into capable systems
that can write code, control browsers, and perform advanced question answering (Comanici et al.,
2025). These advances have been driven by improving inference, planning, and tool use, as shown
by benchmarks emphasizing logical reasoning and multi-step actions. Yet a fundamental capability,
memory, remains largely underexplored. Memory allows LLMs to maintain state across interactions,
accumulate experience, and adapt strategies over time. Recent studies have introduced memory
modules that track dialogue histories through compression, indexing, or retrieval (Maharana et al.,
2024), improving conversational recall and personalization. However, most of these systems only
reuse static dialogue context rather than learning from experience to improve future reasoning or
decision-making.
Despite these advances, existing LLM memory systems remain largely static, retrieving information
passively rather than evolving through use. Current evaluations test whether models can recall past
context but rarely assess their ability to reuse experience. In essence, agents remember what was
said but not what was learned. Conversational recall retrieves prior facts, whereas experience reuse
abstracts reasoning strategies for future tasks. Without such reuse, models repeatedly solve similar
problems, as long-term assistants often recall context yet fail to adapt across sessions.
Corresponding author(s): twei10@illinois.edu
© 2026 Google DeepMind. All rights reserved
arXiv:2511.20857v2  [cs.CL]  18 May 2026


Evo-Memory: Benchmarking LLM Agent Test-time Learning with Self-Evolving Memory
X=-2, 0.5
Quadratic 
formula
What are the 
solutions for 2x² + 3x 
– 1 = 0?
For 2x² + 3x – 1, 
x = –2, 0.5
Quadratic formula:
–b ±
b² – 4ac
2a
What
How
Conversational 
Recall
Experience 
Reuse
(a) Recall vs. reuse.
Multi-turn task
Single-turn task
Self-Evolving Memory
Q: Solve 𝟐𝒙𝟐−𝟓𝒙+ 𝟏= 𝟎
Identify 
equation
Apply 
quadratic formula
Goal: Putting a cooled 
tomato in the microwave
Tomato is in 
the kitchen
Tomato is 
cooled down
Find the tomato
Put it in the 
microwave
Reuse
Reuse
Goal: Put a green cup with 
a fork in it on the counter.
Cup is on 
the table
Fork is in 
the cup
Find the cup
Put them on 
the counter
Q: Solve 𝟓𝒙𝟐−𝟏𝒙+ 𝟕= 𝟎
Identify 
equation
Apply 
quadratic formula
…
…
…
…
(b) Task settings.
Figure 1 | (a) Conversational recall retrieves past facts (e.g., solutions to 2𝑥2 + 3𝑥−1 = 0), while
experience reuse recalls reasoning strategies (e.g., using the formula). (b) A stateful agent encounters
both multi-turn tasks (e.g., embodied manipulation) and single-turn tasks (e.g., solving equations),
and should learn reusable experiences from past interactions.
Several recent benchmarks have begun examining static adaptation but remain limited in scope.
StreamBench (Wu et al., 2024a) evaluates sequential learning but mainly measures factual retention
without reasoning or trajectory reuse. LifelongBench (Zheng et al., 2025) studies lifelong learning
across environments and skills but focuses on retention without modeling memory structure or
updates. Other studies (Hu et al., 2025; Maharana et al., 2024; Wu et al., 2024b) assess long-term
conversational consistency but do not test how agents evolve their memory during deployment.
Together, these efforts highlight a critical gap: while progress has been made on sequential reasoning,
there is still no unified framework for evaluating how different memory methods retrieve, integrate,
and evolve historical strategies in realistic streaming scenarios. Figure 1a illustrates this contrast
between static recall and cumulative improvement through self-evolving memory.
To bridge this gap, we introduce Evo-Memory, a comprehensive streaming benchmark and
framework for evaluating self-evolving memory in LLM agents. Figure 1b illustrates how a self-evolving
agent reuses prior experiences across both multi-turn interactive tasks and single-turn reasoning
tasks. Evo-Memory restructures datasets into sequential task streams, requiring models to retrieve,
adapt, and evolve memory after each interaction. The benchmark covers both multi-turn goal-oriented
environments and single-turn reasoning or problem-solving tasks, explicitly testing whether LLMs can
accumulate knowledge and refine strategies during deployment, a process we term test-time evolution.
We unify and implement over ten representative memory modules, including retrieval-based, workflow,
and hierarchical memory systems, to study their adaptation behavior. To further examine experience
reuse, we introduce ExpRAG, a simple retrieval-based baseline that leverages prior task experiences,
and further develop ReMem, an advanced action–think–memory refine pipeline that tightly integrates
reasoning, action, and memory updates for continual improvement.
In summary, our contributions are threefold:
• Benchmark: We present Evo-Memory, a streaming benchmark that evaluates LLM agents’ ability
to perform test-time evolution across diverse multi-turn and single-turn tasks, bridging the gap
between conversational recall and experience reuse.
• Framework: We provide a unified evaluation framework with memory-centric metrics for analyzing
adaptation, efficiency, and stability, and will release all code and configurations for reproducibility.
• Analysis and Insights: We introduce ExpRAG, a simple retrieval-based baseline for experience
2


Evo-Memory: Benchmarking LLM Agent Test-time Learning with Self-Evolving Memory
reuse, and ReMem, an action–think–memory refine pipeline that unifies reasoning, action, and
memory for continual improvement, informing future designs of memory.
2. Related Work
In this section, we review existing works on test-time learning and self-evolving memory.
2.1. Test-time Learning
Test-time learning (TTL) builds upon early work on test-time adaptation (TTA) (Wang et al., 2020;
Zhang et al., 2022), which enables models to adjust to distribution shifts during deployment. Recent
advances extend TTA toward continuous self-improvement (Iwasawa and Matsuo, 2021; Liu et al.,
2021), allowing models to refine their behavior through online optimization. Recent agent-based
studies operationalize such continual improvement via reflection, planning, and self-evolution. Works
like (He et al., 2025a,b; Ma et al., 2024; Park et al., 2023; Shinn et al., 2023; Wang et al., 2023)
and newer frameworks, including (Yang et al., 2024) demonstrate how agents autonomously revise
plans, synthesize feedback, and co-evolve (Gao et al., 2025). These advances mark a shift from static
adaptation toward adaptive, self-improving agents capable of continual learning during deployment.
2.2. Self-evolving Memory
Early LLM memory systems primarily served as passive storage, maintaining recent dialogues or
retrieved facts to compensate for limited context windows (Asai et al., 2024a; Lewis et al., 2020; Liu,
2022; Packer et al., 2023; Zhong et al., 2023). Subsequent studies introduced richer management
mechanisms, including differentiable read–write controllers (Liang et al., 2023; Modarressi et al.,
2023) and evaluations under realistic conversational settings (Maharana et al., 2024; Wu et al.,
2024b). Beyond static buffers, recent work explores policy-driven control, where the model is explicitly
optimized to decide what to store, retrieve, or overwrite (Li et al., 2025; Xu et al., 2025; Yan et al., 2025;
Yu et al., 2025; Zhou et al., 2025). Meanwhile, structured memory representations have emerged
to organize experiences into relational or procedural forms, as in RepoGraph (Ouyang et al., 2024),
MEM0 (Chhikara et al., 2025), Zep (Rasmussen et al., 2025), and Dynamic Cheatsheets (Suzgun
et al., 2025). However, there remains no unified evaluation setting and framework for self-evolving
memory, the ability to reuse and adapt experiences across tasks. Evo-Memory builds on this trajectory
by benchmarking how LLMs not only store and recall but also evolve, reorganize, and reuse memory
under streaming task settings.
3. Evo-Memory: Evaluating Self-Evolving Memory in LLM Agents
Existing evaluations of LLMs often treat memory as static recall, overlooking its role in continual
adaptation. Evo-Memory provides a unified benchmark to study self-evolving memory, where agents
retrieve, integrate, and update knowledge over time. As illustrated in Figure 2, the left side shows the
test-time evolution process, and the right side outlines the ReMem agent with three modules: Think,
Act, and Refine Memory. We first formalize the problem setting, then describe two representative
implementations, ExpRAG and ReMem, used to instantiate the benchmark.
3.1. Problem Formulation
We formalize a general memory-augmented agent as a tuple (F, U, R, C), where F is the base LLM,
U is the memory update pipeline, R is the retrieval module, and C is the contextual construction
3


Evo-Memory: Benchmarking LLM Agent Test-time Learning with Self-Evolving Memory
LLM Response
Synthesis
Search
Synthesis
Evolve
Search
Synthesis
Evolve
Search
Evolve
LLM Response
LLM Response
Think
Generate reasoning: 
Internal thinking and 
task decomposition
Refine Memory
Explore memory 
utilization: retrieve, 
prune, organize
Act
Execute an operation or 
produce an final 
response
ReMem 
Agent
Memory
Environment
(a) Test-time Evolution Setting
(b) Illustrative Think-Act-Refine Loop 
Figure 2 | Overview of the ReMem agent framework. Left: Test-time evolution process where the
agent iteratively searches, synthesizes, and evolves its memory across multiple tasks. Right: Agent
architecture with three core modules, Think (reasoning and decomposition), Refine Memory (retrieve,
prune, organize), and Act (execution), that interact with the environment and learned memory.
mechanism that transforms retrieved content into the final working context. In our setting, the agent
processes a sequence of inputs {𝑥1, 𝑥2, . . . , 𝑥𝑇}, and the memory state 𝑀𝑡evolves with the history. At
time 𝑡, the agent receives an input 𝑥𝑡, maintains an evolving memory 𝑀𝑡, retrieves relevant elements
R(𝑀𝑡, 𝑥𝑡), constructs a contextualized prompt
𝐶𝑡= C(𝑥𝑡, R(𝑀𝑡, 𝑥𝑡)),
and produces an output
ˆ𝑦𝑡= F(𝐶𝑡).
This abstraction unifies a wide spectrum of existing memory mechanisms, from retrieval-augmented
generation to dynamic, hierarchical, and workflow-based memories, under a single iterative formula-
tion.
Search.
Given the current input 𝑥𝑡, the agent first retrieves relevant memory entries:
𝑅𝑡= R(𝑀𝑡, 𝑥𝑡),
where R can represent similarity search, index-based lookup, or attention over stored embeddings.
This step captures memory access policies across different algorithms.
Synthesis.
The agent restructures the retrieved information 𝑅𝑡into a working context tailored to
the current input 𝑥𝑡. This step may involve forming a structured prompt (Wang et al., 2024), selecting
key memory items (Chhikara et al., 2025; Xu et al., 2025), or merging retrieved content (Suzgun
et al., 2025) into a short summary. We denote the resulting context as ˜𝐶𝑡= C(𝑥𝑡, 𝑅𝑡), and the final
output is
ˆ𝑦𝑡= F( ˜𝐶𝑡).
4


Evo-Memory: Benchmarking LLM Agent Test-time Learning with Self-Evolving Memory
Evolve.
After obtaining ˆ𝑦𝑡, the agent constructs a new memory entry 𝑚𝑡= ℎ(𝑥𝑡, ˆ𝑦𝑡, 𝑓𝑡) that captures
the current step’s experience together with the feedback 𝑓𝑡, such as whether the task was completed.
The memory is then updated via:
𝑀𝑡+1 = U(𝑀𝑡, 𝑚𝑡).
Different algorithms instantiate 𝑈differently, for example, direct append for retrieval-based memories,
summarization or compression for long-term storage, or replacement for bounded-capacity stores.
This unified formulation abstracts the essential cycle of retrieval, synthesis, and evolution underlying
all memory-based agents.
Dataset Preparation.
Evo-Memory restructures conventional static datasets into streaming task
sequences, enabling evaluation of how LLMs reuse and evolve memory over time. Each dataset can
thus be transformed into a sequence 𝜏= {(𝑥1, 𝑦1), . . . , (𝑥𝑇, 𝑦𝑇)}, forming a ground-truth trajectory
in which earlier tasks provide essential information or strategies for later ones. At each step 𝑡, the
agent processes input 𝑥𝑡, retrieves and synthesizes memory, produces prediction ˆ𝑦𝑡, and updates the
memory state 𝑀𝑡, yielding the predicted trajectory:
(𝑥1, ˆ𝑦1, 𝑀1) →(𝑥2, ˆ𝑦2, 𝑀2) →· · · →(𝑥𝑇, ˆ𝑦𝑇, 𝑀𝑇).
This design transforms static benchmarks into interactive evaluation streams that explicitly probe an
LLM’s ability to accumulate, adapt, and refine knowledge during deployment.
3.2. ExpRAG: Experience Retrieval and Aggregation
As a simple baseline and extension, we define ExpRAG, a task-level retrieval-augmented agent. Each
memory entry 𝑚𝑖= 𝑆(𝑥𝑖, ˆ𝑦𝑖, 𝑓𝑖) encodes a structured experience text with template 𝑆. At step 𝑡, the
agent retrieves 𝑘similar experiences from memory according to a retrieval score 𝜙:
𝑅𝑡= Top-𝑘𝑚𝑖∈𝑀𝑡𝜙(𝑥𝑡, 𝑚𝑖).
The model conditions on these retrieved examples following the in-context learning principle:
ˆ𝑦𝑡= F(𝑥𝑡, 𝑅𝑡),
and appends the new experience to memory:
𝑀𝑡+1 = 𝑀𝑡∪{(𝑥𝑡, ˆ𝑦𝑡, 𝑓𝑡)}.
ExpRAG thus performs one-shot experience reuse through retrieval and aggregation. It captures
how simple memory-based extensions of in-context learning behave but lacks iterative reasoning or
adaptive refinement during inference.
3.3. ReMem: Synergizing Reasoning, Acting, and Memory
We propose ReMem, a simple yet effective framework that unifies reasoning, action, and memory
refinement within a single decision loop. Unlike conventional retrieval-augmented or ReAct-style
methods that treat memory as static context, ReMem introduces a third dimension of memory reasoning,
allowing the agent to actively evaluate, reorganize, and evolve its own memory during problem solving.
At each step 𝑡, given the current input 𝑥𝑡, memory state 𝑀𝑡, and previous reasoning traces 𝑜1:𝑛−1
𝑡
at this step, the agent selects one of the operations:
𝑎𝑛
𝑡∈{Think, Act, Refine}.
5


Evo-Memory: Benchmarking LLM Agent Test-time Learning with Self-Evolving Memory
It then performs the operation and transitions according to:
𝑜𝑛
𝑡= Agent(𝑥𝑡, 𝑀𝑡, 𝑎𝑛
𝑡),
where 𝑜𝑛
𝑡denotes the output generated at step 𝑡after 𝑛operations, such as an intermediate reasoning
trace, an external action, and memory refine thoughts.
Specifically, Think produces internal reasoning traces that help decompose the task and guide
subsequent actions; Act executes an operation in the environment or outputs a response observable to
the user; Refine performs meta-reasoning over memory, which exploiting useful experiences, pruning
noise, and reorganizing 𝑀𝑡, to better support future reasoning and action. Within each step, the agent
may perform multiple rounds of Think and Refine, and the step terminates once an Act operation
is selected. This induces a Markov decision process where the state at step 𝑡after 𝑛operations is
𝑠𝑛
𝑡= (𝑥𝑡, 𝑀𝑡, 𝑜1:𝑛−1
𝑡
), the action space is {Think, Act, Refine}, and the transition dynamics are given by
the Agent operator together with the environment response. Depending on the task, the Act-output
of step 𝑡may serve as the final answer for single-step tasks or as an intermediate result in multi-step
settings, where the process continues until the overall task is completed.
This unified formulation expands the action space of ReAct-style (Yao et al., 2023) agents by
introducing an explicit memory reasoning mechanism. Through this extension, memory becomes an
adaptive component that interacts with reasoning in real time rather than remaining a passive context.
Under this view, the entire decision loop can also be interpreted as a Markov process, where the state
encapsulates the current input, memory state, and ongoing reasoning traces. Such integration yields
a lightweight yet powerful paradigm for continual adaptation, where the agent learns to reason about
both the task and its own knowledge state. By coupling reflection with memory evolution, ReMem
establishes a new standard for adaptive, self-improving LLM agents.
4. Experiments
In this section, we evaluate leading LLMs on the Evo-Memory benchmark under our unified test-time
learning pipeline, focusing on five key research questions (RQs):
• RQ1: How do LLM agents perform on Evo-Memory across domains and task types, and does
ReMem enhance their test-time learning ability?
• RQ2: What factors influence the effectiveness of memory in different tasks, and how does experience
reuse improve task efficiency?
• RQ3: How does task sequence difficulty (e.g., easy vs. hard trajectories) affect memory adaptation
and generalization?
• RQ4: How do varying feedback types impact learning dynamics and memory refinement?
4.1. Experimental Setup
Evo-Memory evaluates memory mechanisms under realistic streaming multi-task conditions. In what
follows, we describe the benchmark datasets, metrics, and the methods compared.
4.1.1. Datasets
Evo-Memory is evaluated on a diverse suite of datasets spanning factual knowledge, reasoning,
mathematics, programming, and goal-oriented interaction. For factual and reasoning ability, we
include MMLU-Pro (Zheng et al., 2024) and GPQA-Diamond (Rein et al., 2024), which test multi-
disciplinary and graduate-level reasoning. For mathematical problem solving, we use AIME-24
6


Evo-Memory: Benchmarking LLM Agent Test-time Learning with Self-Evolving Memory
and AIME-25 (HuggingFaceH4, 2024), containing math challenges requiring symbolic reasoning
and exact-match evaluation. For tool-use and API grounding, we include ToolBench (Patil et al.,
2023). For multi-turn and goal-oriented interaction, we adopt Alf World (Shridhar et al., 2021),
BabyAI (Chevalier-Boisvert et al., 2019), ScienceWorld (Wang et al., 2022), and PDDL tasks (Vallati
et al., 2015). All methods are evaluated under the same search–predict–evolve loop
(𝑥𝑡, 𝑀𝑡)
search
−−−−−→𝑅𝑡
synthesis
−−−−−−−→ˆ𝑦𝑡
evolve
−−−−−→𝑀𝑡+1,
with preferred prompting templates, and configurations. Feedback 𝑓𝑡is considered the correctness
signal.
4.1.2. Methods
We benchmark a broad range of agents and memory architectures instantiated on two strong LLM
backbones: the Gemini-2.5 series (Comanici et al., 2025) (Flash, Flash-Lite, and Pro) and
the Claude family (Anthropic, 2025) (3.5-Haiku and 3.7-Sonnet). The evaluated methods are
grouped into four categories: (1) Agent pipelines without procedural memory, including ReAct (Yao
et al., 2023) and Amem (Xu et al., 2025), which rely on context instead of learned procedures.; (2)
Adaptive agentic memory methods, such as SelfRAG (Asai et al., 2024c), MemOS (Li et al., 2025),
Mem0 (Chhikara et al., 2025), and LangMem (LangChain contributors), which support dynamic
retrieval and continual updates; (3) Memory-based agents for procedural knowledge, including
Dynamic Cheatsheet (DC) (Suzgun et al., 2025) with two variants Cumulative (Cu) and Synthesis (RS),
and Agent Workflow Memory (AWM) (Wang et al., 2024), which emphasize reusable workflows and
task strategies; and (4) Proposed evolving-memory framework, comprising ExpRecent, ExpRAG,
and ReMem, which unify reasoning, action, and memory refinement in a self-evolving loop. All
methods are evaluated under a unified search–predict–evolve protocol to isolate the effects of memory
design. Implementation and prompting details are provided in Appendix A. We exclude systems such
as MemoryGpt (Zhong et al., 2023) and MemoryBank (Zhong et al., 2023) that target factual recall
only, as well as methods incompatible with embodied environments (e.g., MemOS and LangMem)
from multi-turn evaluations.
4.2. Experimental Results
Below are the conducted experiments to answer the proposed research questions.
4.2.1. Analysis of Results (RQ1)
Tables 1a and 1b summarize the results across single-turn and multi-turn settings. Overall, Evo-
Memory demonstrates that self-evolving memory architectures provide consistent improvements. In
single-turn reasoning and QA benchmarks (AIME-24/25, GPQA, MMLU-Pro, ToolBench), evolving-
memory methods show consistent improvements, with ReMem achieving 0.65 average exact match
and 0.85/0.71 API accuracy under Gemini-2.5 Flash. Adaptive retrieval methods enhance factual
grounding, yet only evolving systems maintain consistent gains through iterative refinement. Agents
with procedural knowledge perform well on structured domains such as AIME but lag in scientific
reasoning and tool use, showing limited flexibility. ExpRAG serves as a simple yet highly effective
baseline, outperforming several more complex designs. While improvements in single-turn settings
are moderate, the overall trend remains consistent across datasets and model families.
In multi-turn reasoning environments (Alf World, BabyAI, PDDL, ScienceWorld), ReMem and
ExpRAG achieve strong and stable performance on both Gemini-2.5 and Claude backbones, reach-
ing 0.92/0.96 on BabyAI and 0.95/0.62 on ScienceWorld. These results indicate that continual
7


Evo-Memory: Benchmarking LLM Agent Test-time Learning with Self-Evolving Memory
LLM
Backbone
Method
Exact Match ↑
API / Acc. ↑
AIME24
AIME25
GPQA
MMLU-Pro
ToolBen.
Avg. ↑
Eco.
Eng.
Philo.
Claude
3.7
Sonnet
Baseline
0.17
0.13
0.55
0.84
0.63
0.78
0.76/0.62
0.54
History
0.13
0.23
0.56
0.85
0.64
0.78
0.76/0.61
0.55
ReAct
0.17
0.10
0.57
0.84
0.63
0.76
0.76/0.61
0.54
Amem
0.27
0.17
0.54
0.83
0.63
0.79
0.77/0.63
0.56
SelfRAG
0.20
0.10
0.58
0.84
0.65
0.77
0.77/0.63
0.55
MemOS
0.17
0.20
0.55
0.84
0.64
0.76
0.76/0.62
0.55
Mem0
0.20
0.13
0.58
0.84
0.62
0.77
0.76/0.61
0.55
LangMem
0.10
0.13
0.53
0.77
0.56
0.66
0.77/0.63
0.49
DC-Cu
0.17
0.23
0.57
0.79
0.52
0.65
0.77/0.62
0.52
DC-RS
0.20
0.20
0.62
0.79
0.52
0.60
0.77/0.62
0.52
AWM
0.03
0.03
0.53
0.80
0.56
0.72
0.76/0.62
0.48
ExpRecent
0.13
0.20
0.61
0.86
0.63
0.78
0.82/0.66
0.56
ExpRAG
0.17
0.17
0.70
0.85
0.67
0.80
0.88/0.72
0.59
ReMem
0.13
0.13
0.67
0.86
0.65
0.80
0.87/0.71
0.58
Gemini
2.5
Flash
Baseline
0.47
0.47
0.48
0.83
0.46
0.75
0.71/0.61
0.59
History
0.60
0.47
0.43
0.84
0.42
0.78
0.62/0.54
0.58
ReAct
0.30
0.27
0.05
0.64
0.16
0.54
0.64/0.57
0.37
Amem
0.70
0.57
0.52
0.83
0.42
0.72
0.72/0.60
0.63
SelfRAG
0.50
0.47
0.46
0.83
0.45
0.75
0.72/0.61
0.59
MemOS
0.47
0.47
0.50
0.82
0.46
0.75
0.71/0.61
0.59
Mem0
0.50
0.47
0.45
0.83
0.46
0.74
0.71/0.61
0.59
LangMem
0.43
0.50
0.53
0.79
0.39
0.71
0.68/0.57
0.57
DC-Cu
0.60
0.40
0.48
0.79
0.44
0.69
0.70/0.59
0.58
DC-RS
0.53
0.37
0.48
0.80
0.42
0.69
0.68/0.57
0.56
AWM
0.50
0.37
0.49
0.79
0.43
0.72
0.71/0.59
0.56
ExpRecent
0.47
0.47
0.42
0.83
0.39
0.75
0.78/0.66
0.58
ExpRAG
0.43
0.47
0.42
0.83
0.43
0.78
0.87/0.73
0.60
ReMem
0.60
0.53
0.51
0.85
0.46
0.79
0.85/0.71
0.65
(a) Single-turn reasoning and QA results.
LLM
Backbone
Method
Alf World
BabyAI
PDDL
ScienceWorld
Avg.
S
P
S
P
S
P
S
P
S
P
Gemini
2.5
Flash
Baseline
0.12
0.34
0.61
0.71
0.12
0.20
0.24
0.59
0.27
0.46
History
0.28
0.60
0.52
0.64
0.08
0.15
0.31
0.71
0.30
0.53
ReAct
0.24
0.56
0.48
0.63
0.22
0.33
0.34
0.71
0.32
0.56
Amem
0.25
0.59
0.53
0.64
0.10
0.16
0.36
0.74
0.31
0.53
SelfRAG
0.25
0.59
0.52
0.65
0.08
0.16
0.34
0.74
0.30
0.54
Mem0
0.27
0.61
0.54
0.66
0.10
0.19
0.32
0.70
0.31
0.54
DC-Cu
0.25
0.59
0.53
0.64
0.08
0.17
0.29
0.71
0.29
0.53
DC-RS
0.27
0.60
0.53
0.66
0.07
0.15
0.33
0.73
0.30
0.54
AWM
0.26
0.59
0.52
0.64
0.08
0.16
0.33
0.73
0.30
0.53
ExpRecent
0.37
0.65
0.53
0.64
0.13
0.22
0.53
0.83
0.39
0.59
ExpRAG
0.59
0.79
0.56
0.65
0.17
0.27
0.53
0.81
0.46
0.63
ReMem
0.66
0.81
0.53
0.61
0.22
0.33
0.58
0.81
0.50
0.64
Claude
3.7
Sonnet
Baseline
0.18
0.49
0.51
0.66
0.17
0.39
0.10
0.53
0.24
0.52
History
0.50
0.73
0.48
0.66
0.65
0.85
0.32
0.74
0.49
0.74
ReAct
0.51
0.75
0.57
0.72
0.75
0.91
0.44
0.77
0.57
0.79
Amem
0.48
0.73
0.46
0.64
0.62
0.84
0.33
0.73
0.47
0.73
SelfRAG
0.52
0.75
0.46
0.64
0.65
0.84
0.31
0.74
0.49
0.74
Mem0
0.51
0.74
0.48
0.66
0.65
0.84
0.37
0.76
0.50
0.75
DC-Cu
0.50
0.74
0.50
0.67
0.62
0.84
0.33
0.75
0.49
0.75
DC-RS
0.50
0.74
0.52
0.68
0.62
0.84
0.34
0.74
0.50
0.75
AWM
0.49
0.73
0.53
0.68
0.60
0.82
0.34
0.74
0.49
0.74
ExpRecent
0.66
0.83
0.63
0.73
0.53
0.76
0.49
0.82
0.58
0.79
ExpRAG
0.74
0.89
0.62
0.72
0.72
0.89
0.46
0.76
0.63
0.82
ReMem
0.92
0.96
0.73
0.83
0.83
0.95
0.62
0.89
0.78
0.91
(b) Multi-turn embodied reasoning results.
Table 1 | Cross-benchmark results of diverse memory architectures across single-turn and multi-turn
tasks. Left: single-turn reasoning and question answering results. Right: multi-turn embodied
reasoning results.
reflection and refinement substantially improve procedural knowledge accumulation. Performance
gains are notably larger in multi-turn settings, underscoring that continual adaptation becomes
increasingly valuable as task horizons lengthen. While many baselines enhance retrieval grounding,
they struggle to reuse long-horizon experiences and often falter in open-ended environments. Notably,
lightweight variants such as ExpRecent and ExpRAG still perform competitively despite their
simplicity, suggesting that explicit task-level utilization during test-time evolution is both promising
and underexplored.
Across all experiments, evolving-memory methods consistently improve performance on various
backbones and tasks. These results highlight task-level memory utilization and continual reorganiza-
tion as promising directions for evolving-memory agents. Additional results across more LLM families
are reported in Appendix B.1. Smaller models benefit most, indicating that test-time refinement is
also an effective way to enhance lightweight LLMs.
4.2.2. Analysis of Memory Improvement (RQ2)
Figure 3 shows that ReMem’s improvement strongly correlates with within-dataset task similarity
(Pearson 𝑟= 0.717 on Gemini 2.5 Flash and 𝑟= 0.563 on Claude 3.7 Sonnet). Task similarity is
measured by computing the average cosine distance between each task embedding and its dataset
cluster center, where embeddings are obtained from the retriever encoder. A smaller average distance
indicates higher intra-dataset coherence and thus stronger structural similarity. Tasks with higher
embedding cluster ratios, such as PDDL and Alf World, yield larger gains, suggesting that recurring
task structures facilitate memory reuse and generalization. In contrast, more diverse or low-similarity
datasets like AIME-25 or GPQA show smaller gains, reflecting limited transferable experiences. These
findings highlight the importance of embedding organization and semantic overlap in driving effective
memory evolution. Further analysis of memory pruning rates can be found in Appendix B.2.
8


Evo-Memory: Benchmarking LLM Agent Test-time Learning with Self-Evolving Memory
Figure 3 | ReMem performance gain over history baseline versus within-dataset task similarity.
Direction
Method
Alf World
ScienceWorld
Avg.
S
P
S
P
S
P
Base
0.50
0.73
0.32
0.74
0.41
0.74
Easy→Hard
ExpRecent
0.66
0.82
0.48
0.83
0.57
0.83
ExpRAG
0.77
0.87
0.37
0.71
0.57
0.79
ReMem
0.91
0.96
0.63
0.88
0.77 0.92
Hard→Easy
ExpRecent
0.72
0.85
0.47
0.80
0.60
0.83
ExpRAG
0.87
0.92
0.51
0.81
0.69
0.87
ReMem
0.94
0.97
0.68
0.90
0.81 0.94
Table 2 | Comparison of memory-based agents under different sequence difficulty directions.
Easy→Hard and Hard→Easy indicate task order transitions.
Figure 4 compares step efficiency across four environments. Evolving-memory methods consistently
require fewer steps, with ReMem achieving the largest and most stable reductions (e.g., from 22.6
to 11.5 steps on Alf World). Lightweight methods such as ExpRAG and ExpRecent also perform
competitively, indicating that simple task-level evolution substantially improves efficiency. Overall,
continual refinement leads to more focused and efficient reasoning.
4.2.3. Task Sequence: Easy v.s. Hard (RQ3)
Table 2 examines how memory-based agents adapt to changes in task difficulty. Baseline methods
exhibit substantial variation across task sequences, indicating limited robustness under distribution
shifts. In contrast, evolving-memory agents, particularly ReMem, maintain strong and consistent
performance in both directions, reaching up to 0.94/0.97 success and progress in the Hard→Easy
setting. This asymmetry across sequences also suggests that successful experiences from harder tasks
are more transferable, contributing to the improved performance of ReMem. Overall, these results
show that continual reflection helps retain transferable knowledge as task complexity varies, and
highlight the importance of task sequence design for fair evaluation and effective learning.
4.2.4. Analysis of Feedback (RQ4)
Table 3 evaluates agent performance when both successful and failed experiences are stored in
memory. Baseline methods suffer notable degradation under unfiltered failures, indicating that
naive memory accumulation introduces noise and hinders retrieval. In contrast, evolving-memory
9


Evo-Memory: Benchmarking LLM Agent Test-time Learning with Self-Evolving Memory
Figure 4 | Average steps to complete tasks across four benchmarks. We compare four methods: History,
ExpRecent, ExpRAG, and ReMem. Lower is better.
Model
Method
Alf World
ScienceWorld
Avg.
S
P
S
P
S
P
Claude 3.7 Sonnet
Amem
0.49
0.73
0.31
0.74
0.40
0.74
SelfRAG
0.47
0.73
0.34
0.73
0.41
0.73
Mem0
0.49
0.73
0.36
0.74
0.43
0.74
DC-Cu
0.52
0.75
0.34
0.73
0.43
0.74
DC-RS
0.51
0.74
0.38
0.74
0.45
0.74
AWM
0.55
0.76
0.32
0.72
0.44
0.74
ExpRecent
0.62
0.80
0.34
0.74
0.48
0.77
ExpRAG
0.76
0.90
0.27
0.63
0.52
0.77
ReMem
0.92
0.96
0.69
0.91
0.81
0.94
Gemini 2.5 Flash
Amem
0.22
0.57
0.39
0.75
0.31
0.66
SelfRAG
0.25
0.58
0.36
0.71
0.31
0.65
Mem0
0.25
0.59
0.34
0.71
0.30
0.65
DC-Cu
0.20
0.56
0.36
0.72
0.28
0.64
DC-RS
0.21
0.57
0.36
0.71
0.29
0.64
AWM
0.19
0.56
0.36
0.74
0.28
0.65
ExpRecent
0.22
0.57
0.59
0.86
0.41
0.72
ExpRAG
0.25
0.60
0.51
0.78
0.38
0.69
ReMem
0.57
0.76
0.50
0.75
0.54
0.76
Table 3 | Results with both successful and failed task experiences on Alf World and ScienceWorld.
Bold numbers denote the best results per metric.
approaches, particularly ReMem, remain robust by actively refining stored experiences, achieving
the best overall success and progress rates across both Claude and Gemini backbones. These results
highlight the importance of selective utilization and memory refinement for stable test-time adaptation,
and motivate future work on failure-aware memory evolution.
5. Conclusion
Self-evolving memory is a fundamental yet underexplored aspect of LLM capability. While prior work
centers on static conversational recall, it overlooks how models accumulate and reuse experience
across evolving task streams. Evo-Memory fills this gap by transforming static datasets into streaming
trajectories, systematically evaluating how LLMs retrieve, adapt, and refine memory through interac-
tion. Our results show that memory can substantially enhance performance but remains fragile in
stability and procedural reuse. To foster progress, we introduce ExpRAG for experience retrieval and
ReMem for interleaving reasoning, action, and memory updates. We hope Evo-Memory serves as a
unified platform for building LLMs with reliable and continually improving memory.
10


Evo-Memory: Benchmarking LLM Agent Test-time Learning with Self-Evolving Memory
References
Anthropic. Introducing Claude 4. https://www.anthropic.com/news/claude-4, 2025.
A. Asai, Z. Wu, Y. Wang, A. Sil, and H. Hajishirzi. Self-rag: Learning to retrieve, generate, and critique
through self-reflection. In The Twelfth International Conference on Learning Representations, ICLR
2024, Vienna, Austria, May 7-11, 2024. OpenReview.net, 2024a. URL https://openreview.
net/forum?id=hSyW5go0v8.
A. Asai, Z. Wu, Y. Wang, A. Sil, and H. Hajishirzi. Self-rag: Learning to retrieve, generate, and critique
through self-reflection. In International conference on learning representations, volume 2024, pages
9112–9141, 2024b.
A. Asai, Z. Wu, Y. Wang, A. Sil, and H. Hajishirzi. Self-rag: Learning to retrieve, generate, and critique
through self-reflection. 2024c.
J. Chen, S. Xiao, P. Zhang, K. Luo, D. Lian, and Z. Liu. M3-embedding: Multi-linguality, multi-
functionality, multi-granularity text embeddings through self-knowledge distillation. In Findings of
the association for computational linguistics: ACL 2024, pages 2318–2335, 2024.
M. Chevalier-Boisvert, D. Bahdanau, S. E.-T. Lahlou, L. Willems, H. Lozano, L. Dassa, S. Kim, J. Pineau,
and A. Courville. Babyai: A platform to study the sample efficiency of grounded language learning.
In International Conference on Learning Representations (ICLR), 2019.
P. Chhikara, D. Khant, S. Aryan, T. Singh, and D. Yadav. Mem0: Building production-ready ai agents
with scalable long-term memory. arXiv preprint arXiv:2504.19413, 2025.
G. Comanici, E. Bieber, M. Schaekermann, I. Pasupat, N. Sachdeva, I. Dhillon, M. Blistein, O. Ram,
D. Zhang, E. Rosen, et al. Gemini 2.5: Pushing the frontier with advanced reasoning, multimodality,
long context, and next generation agentic capabilities. arXiv preprint arXiv:2507.06261, 2025.
H.-a. Gao, J. Geng, W. Hua, M. Hu, X. Juan, H. Liu, S. Liu, J. Qiu, X. Qi, Y. Wu, et al. A survey
of self-evolving agents: What, when, how, and where to evolve on the path to artificial super
intelligence. arXiv preprint arXiv:2507.21046, 2025.
Y. He, R. Li, A. Chen, Y. Liu, Y. Chen, Y. Sui, C. Chen, Y. Zhu, L. Luo, F. Yang, et al. Enabling
self-improving agents to learn at test time with human-in-the-loop guidance. In Proceedings of
the 2025 Conference on Empirical Methods in Natural Language Processing: Industry Track, pages
1625–1653, 2025a.
Y. He, J. Liu, Y. Liu, Y. Li, T. Cao, Z. Hu, X. Xu, and B. Hooi. Evotest: Evolutionary test-time learning
for self-improving agentic systems. arXiv preprint arXiv:2510.13220, 2025b.
Y. Hu, Y. Wang, and J. McAuley.
Evaluating memory in llm agents via incremental multi-turn
interactions. arXiv preprint arXiv:2507.05257, 2025.
HuggingFaceH4.
American invitational mathematics examination (aime) 2024.
https://
huggingface.co/datasets/HuggingFaceH4/aime_2024, 2024.
Y. Iwasawa and Y. Matsuo. Test-time classifier adjustment module for model-agnostic domain general-
ization. Advances in Neural Information Processing Systems, 34:2427–2440, 2021.
LangChain contributors. Langchain. URL https://github.com/langchain-ai/langchain.
11


Evo-Memory: Benchmarking LLM Agent Test-time Learning with Self-Evolving Memory
P. Lewis, E. Perez, A. Piktus, F. Petroni, V. Karpukhin, N. Goyal, H. Küttler, M. Lewis, W. Yih,
T. Rocktäschel, S. Riedel, and D. Kiela. Retrieval-augmented generation for knowledge-intensive NLP
tasks. In H. Larochelle, M. Ranzato, R. Hadsell, M. Balcan, and H. Lin, editors, Advances in Neural
Information Processing Systems 33: Annual Conference on Neural Information Processing Systems 2020,
NeurIPS 2020, December 6-12, 2020, virtual, 2020. URL https://proceedings.neurips.
cc/paper/2020/hash/6b493230205f780e1bc26945df7481e5-Abstract.html.
Z. Li, S. Song, H. Wang, S. Niu, D. Chen, J. Yang, C. Xi, H. Lai, J. Zhao, Y. Wang, et al. Memos:
An operating system for memory-augmented generation (mag) in large language models. arXiv
preprint arXiv:2505.22101, 2025.
X. Liang, B. Wang, H. Huang, S. Wu, P. Wu, L. Lu, Z. Ma, and Z. Li. Scm: Enhancing large language
model with self-controlled memory framework. 2023. URL https://api.semanticscholar.
org/CorpusID:258331553.
J. Liu. LlamaIndex, 11 2022. URL https://github.com/jerryjliu/llama_index.
Y. Liu, P. Kothari, B. Van Delft, B. Bellot-Gurlet, T. Mordan, and A. Alahi. Ttt++: When does
self-supervised test-time training fail or thrive? Advances in Neural Information Processing Systems,
34:21808–21820, 2021.
Y. J. Ma, W. Liang, G. Wang, D.-A. Huang, O. Bastani, D. Jayaraman, Y. Zhu, J. Fan, et al. Eureka:
Human-level reward design via coding large language models. In International conference on
learning Representations, volume 2024, pages 26516–26560, 2024.
A. Maharana, D.-H. Lee, S. Tulyakov, M. Bansal, F. Barbieri, and Y. Fang. Evaluating very long-term
conversational memory of llm agents. In Proceedings of the 62nd Annual Meeting of the Association
for Computational Linguistics (Volume 1: Long Papers), pages 13851–13870, 2024.
A. Modarressi, A. Imani, M. Fayyaz, and H. Schütze. Ret-llm: Towards a general read-write memory for
large language models. ArXiv, abs/2305.14322, 2023. URL https://api.semanticscholar.
org/CorpusID:258841042.
S. Ouyang, W. Yu, K. Ma, Z.-Q. Xiao, Z. Zhang, M. Jia, J. Han, H. Zhang, and D. Yu. Repograph:
Enhancing ai software engineering with repository-level code graph. ArXiv, abs/2410.14684, 2024.
URL https://api.semanticscholar.org/CorpusID:273502041.
C. Packer, V. Fang, S. G. Patil, K. Lin, S. Wooders, and J. Gonzalez. Memgpt: Towards llms as
operating systems. ArXiv, abs/2310.08560, 2023. URL https://api.semanticscholar.
org/CorpusID:263909014.
J. S. Park, C. O’Brien, C. J. Cai, M. R. Morris, P. Liang, and M. S. Bernstein. Generative agents:
Interactive simulacra of human behavior. In ACM Symposium on User Interface Software and
Technology (UIST), 2023.
S. G. Patil, H. Li, T. Zhang, et al. Gorilla: Large language model connected with massive apis. arXiv
preprint arXiv:2305.15334, 2023.
P. Rasmussen, P. Paliychuk, T. Beauvais, J. Ryan, and D. Chalef.
Zep: A temporal knowledge
graph architecture for agent memory.
ArXiv, abs/2501.13956, 2025.
URL https://api.
semanticscholar.org/CorpusID:275907122.
D. Rein, B. L. Hou, A. C. Stickland, J. Petty, R. Y. Pang, J. Dirani, J. Michael, and S. R. Bowman. Gpqa:
A graduate-level google-proof q&a benchmark. In First Conference on Language Modeling, 2024.
12


Evo-Memory: Benchmarking LLM Agent Test-time Learning with Self-Evolving Memory
N. Shinn, F. Cassano, B. Labash, A. Gopinath, K. Narasimhan, and S. Yao. Reflexion: language
agents with verbal reinforcement learning. In Neural Information Processing Systems, 2023. URL
https://api.semanticscholar.org/CorpusID:258833055.
M. Shridhar, J. Thomason, D. Gordon, Y. Bisk, W. Han, R. Mottaghi, L. Zettlemoyer, and D. Fox.
Alfworld: Aligning text and embodied environments for interactive learning. In International
Conference on Learning Representations (ICLR), 2021.
M. Suzgun, M. Yuksekgonul, F. Bianchi, D. Jurafsky, and J. Zou. Dynamic cheatsheet: Test-time
learning with adaptive memory. arXiv preprint arXiv:2504.07952, 2025.
M. Vallati, L. Chrpa, M. Grześ, T. L. McCluskey, M. Roberts, S. Sanner, et al. The 2014 international
planning competition: Progress and trends. Ai Magazine, 36(3):90–98, 2015.
D. Wang, E. Shelhamer, S. Liu, B. Olshausen, and T. Darrell. Tent: Fully test-time adaptation by
entropy minimization. arXiv preprint arXiv:2006.10726, 2020.
G. Wang, Y. Wang, Z. Wu, G. Chen, Z. Huang, H. Zhao, S. Han, V. Koltun, J. Zhu, and K. Lin. Voyager:
An open-ended embodied agent with large language models. In Advances in Neural Information
Processing Systems (NeurIPS), 2023.
Y. Wang, L. Yuan, K. Gopalakrishnan, A. Narayan-Chen, A. Fang, and M. Hausknecht. Scienceworld:
Is your agent smarter than a 5th grader? In NeurIPS, 2022.
Z. Z. Wang, J. Mao, D. Fried, and G. Neubig. Agent workflow memory. ArXiv, abs/2409.07429, 2024.
URL https://api.semanticscholar.org/CorpusID:272592995.
C.-K. Wu, Z. R. Tam, C.-Y. Lin, Y.-N. V. Chen, and H.-y. Lee. Streambench: Towards benchmarking
continuous improvement of language agents. Advances in Neural Information Processing Systems,
37:107039–107063, 2024a.
D. Wu, H. Wang, W. Yu, Y. Zhang, K.-W. Chang, and D. Yu. Longmemeval: Benchmarking chat
assistants on long-term interactive memory. arXiv preprint arXiv:2410.10813, 2024b.
W. Xu, K. Mei, H. Gao, J. Tan, Z. Liang, and Y. Zhang. A-mem: Agentic memory for llm agents. arXiv
preprint arXiv:2502.12110, 2025.
S. Yan, X. Yang, Z. Huang, E. Nie, Z. Ding, Z. Li, X. Ma, J. Bi, K. Kersting, J. Z. Pan, et al. Memory-r1:
Enhancing large language model agents to manage and utilize memories via reinforcement learning.
arXiv preprint arXiv:2508.19828, 2025.
C. Yang, X. Wang, Y. Lu, H. Liu, Q. V. Le, D. Zhou, and X. Chen. Large language models as optimizers.
In International Conference on Learning Representations, volume 2024, pages 12028–12068, 2024.
S. Yao, J. Zhao, D. Yu, N. Du, I. Shafran, K. Narasimhan, and Y. Cao. React: Synergizing reasoning and
acting in language models. In International Conference on Learning Representations (ICLR), 2023.
H. Yu, T. Chen, J. Feng, J. Chen, W. Dai, Q. Yu, Y.-Q. Zhang, W.-Y. Ma, J. Liu, M. Wang, et al.
Memagent: Reshaping long-context llm with multi-conv rl-based memory agent. arXiv preprint
arXiv:2507.02259, 2025.
M. Zhang, S. Levine, and C. Finn. Memo: Test time robustness via adaptation and augmentation.
Advances in neural information processing systems, 35:38629–38642, 2022.
Y. Zhang and T. Math-AI. American invitational mathematics examination (aime) 2025, 2025.
13


Evo-Memory: Benchmarking LLM Agent Test-time Learning with Self-Evolving Memory
J. Zheng, X. Cai, Q. Li, D. Zhang, Z. Li, Y. Zhang, L. Song, and Q. Ma. Lifelongagentbench: Evaluating
llm agents as lifelong learners. arXiv preprint arXiv:2505.11942, 2025.
Y. Zheng, T. Li, H. Li, C. Zhao, J. Wang, Z. Zhang, S. Deng, N. Zhang, and H. Chen. Mmlu-pro:
A more robust and challenging multi-task language understanding benchmark. arXiv preprint
arXiv:2406.01574, 2024.
W. Zhong, L. Guo, Q.-F. Gao, H. Ye, and Y. Wang. Memorybank: Enhancing large language models
with long-term memory. ArXiv, abs/2305.10250, 2023. URL https://api.semanticscholar.
org/CorpusID:258741194.
Z. Zhou, A. Qu, Z. Wu, S. Kim, A. Prakash, D. Rus, J. Zhao, B. K. H. Low, and P. P. Liang. MEM1:
Learning to synergize memory and reasoning for efficient long-horizon agents. arXiv preprint
arXiv:2506.15841, 2025.
14


Evo-Memory: Benchmarking LLM Agent Test-time Learning with Self-Evolving Memory
Appendix
Contents
A
Experimental Details
17
A.1
Datasets . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . .
17
A.2 Configuration . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . .
17
A.3
Evaluation . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . .
18
A.4
Methods . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . .
18
B
Experiments
19
B.1
Additional Experiments . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . .
19
B.2
Additional Analysis of Memory Pruning . . . . . . . . . . . . . . . . . . . . . . . . . .
20
B.3
Cumulative Accuracy Across Tasks and Models . . . . . . . . . . . . . . . . . . . . . .
20
C
Potential Risks
20
D Prompts
22
E
Ethical Considerations and Artifact Documentation
23
E.1
Cite Creators of Artifacts . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . .
23
E.2
Discuss the License for Artifacts . . . . . . . . . . . . . . . . . . . . . . . . . . . . . .
23
E.3
Artifact Use Consistent with Intended Purpose . . . . . . . . . . . . . . . . . . . . . .
23
E.4
Personally Identifying Information or Offensive Content
. . . . . . . . . . . . . . . .
23
E.5
Documentation of Artifacts . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . .
24
F
Statistics for Data
24
F.1
Single-Turn Reasoning Datasets . . . . . . . . . . . . . . . . . . . . . . . . . . . . . .
24
F.2
Multi-Turn Interactive Environments . . . . . . . . . . . . . . . . . . . . . . . . . . .
24
G Computational Experiments
24
G.1 Models and Budgets
. . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . .
25
G.2 Descriptive Statistics . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . .
25
G.3 Parameters for Packages . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . .
25
H AI Assistants in Research or Writing
25
15


Evo-Memory: Benchmarking LLM Agent Test-time Learning with Self-Evolving Memory
I
Limitations
25
16


Evo-Memory: Benchmarking LLM Agent Test-time Learning with Self-Evolving Memory
A. Experimental Details
Evo-Memory evaluates memory mechanisms under realistic streaming multi-task conditions. In what
follows, we describe the benchmark datasets, metrics, configurations and the methods compared in
details.
A.1. Datasets
We evaluate our approach on a diverse suite of benchmarks that span factual knowledge, reasoning,
mathematics, programming, and goal-oriented interaction.
We first introduce a suite of single-turn datasets designed to evaluate diverse reasoning abilities.
MMLU-Pro (Zheng et al., 2024) extends the original MMLU benchmark with stronger robustness
and challenge by filtering data leakage, reducing ambiguity, and introducing more difficult questions
across domains such as engineering, philosophy, and economics, making it a more reliable testbed
for assessing multi-disciplinary reasoning. GPQA-Diamond (Rein et al., 2024) is a graduate-level
benchmark featuring expert-written, “Google-proof” questions in physics and related sciences, with its
Diamond split being the most challenging and requiring rigorous multi-step reasoning. AIME-24 and
AIME-25 (HuggingFaceH4, 2024; Zhang and Math-AI, 2025) consist of Olympiad-style mathematics
problems from the 2024 and 2025 American Invitational Mathematics Examinations, testing symbolic
manipulation and problem-solving under strict exact-match criteria. Finally, ToolBench (Patil et al.,
2023) assesses a model’s ability to identify and configure external APIs, reflecting practical tool-use
capabilities.
We then evaluate on a suite of multi-turn, goal-oriented benchmarks designed to evaluate mem-
ory in embodied and interactive environments. It includes several representative domains: Alf-
World (Shridhar et al., 2021) for household instruction following, BabyAI (Chevalier-Boisvert et al.,
2019) for grounded navigation and compositional reasoning, ScienceWorld (Wang et al., 2022) for
open-ended scientific experimentation, and PDDL tasks (Vallati et al., 2015) for symbolic planning.
Together, these environments emphasize long-horizon reasoning, sequential decision-making, and
the use of accumulated experience to achieve complex goals.
Together, these datasets form a comprehensive benchmark suite that evaluates factual recall,
domain expertise, mathematical reasoning, and procedural memory in interactive settings. This
diversity enables a unified evaluation of both static and evolving capabilities, reflecting how LLMs
learn, act, and adapt across academic and real-world scenarios.
A.2. Configuration
For efficient retrieval and fair comparison across methods, we utilize the BAAI/bge-base-en-v1.5 (Chen
et al., 2024) encoder as the retriever to index both queries and memory items. During inference, the
current question is encoded as a query and compared with all stored memory embeddings, retrieving
the top-𝑘most relevant items (default 𝑘= 4) for contextual augmentation. This setting ensures
a consistent retrieval budget across all methods. For efficiency, retrieved texts and task inputs are
truncated to fit within the same prompt length constraint used by the generation models.
While all baselines adopt the same retrieval configuration, certain methods (e.g., Self-RAG,
ReMem) introduce additional reasoning modules that determine whether to retrieve and what to
retrieve at each step. These adaptive behaviors operate on top of the same retrieval pool to ensure
comparability.
Across all experiments, we maintain a unified task sequence ordering within each dataset, ensuring
17


Evo-Memory: Benchmarking LLM Agent Test-time Learning with Self-Evolving Memory
consistent memory evolution dynamics for all models. Unless otherwise specified, retrieval and
generation operate within the same pipeline, and the retrieved items are appended to the prompt
following the order of relevance, from most to least similar.
We benchmark a broad range of agents and memory architectures instantiated on two strong
LLM backbones: the Gemini-2.5 series (Comanici et al., 2025) (Flash, Flash-Lite, and Pro) and
the Claude family (Anthropic, 2025) (3.5-Haiku and 3.7-Sonnet).
A.3. Evaluation
Evo-Memory evaluates both task performance and memory quality along four key dimensions:
• Answer accuracy. Evaluates whether the LLM produces correct outputs across tasks, reflecting its
ability to incorporate past experiences into inference.
• Success rate. Measures whether the LLM agent successfully completes task goals, indicating its
overall effectiveness in interactive or goal-oriented settings.
• Step efficiency. Tracks the number of steps required to complete a goal, assessing whether memory
usage enables concise and scalable reasoning.
• Sequence robustness. Examines whether the LLM maintains consistent knowledge and perfor-
mance across varying task orders, reflecting its ability to stably reuse prior experiences.
A.4. Methods
We benchmark Evo-Memory with a wide spectrum of agent and memory architectures to study how
different designs impact test-time memory evolution. All methods are instantiated on two strong
LLM backbones: Gemini-2.5 (Comanici et al., 2025) and Claude-3.5/3.7 (Anthropic, 2025). Our
comparisons isolate the impact of memory architecture and update strategy. Differences in backbone
capability are not the focus of the study. We group the evaluated approaches into four major families:
Agent Pipelines without Procedural Memory.
ReAct (Yao et al., 2023) serves as a representative
reasoning–action pipeline, where memory is limited to the immediate context. It generates interleaved
reasoning traces and tool calls but does not explicitly store or evolve information. Amem Xu et al.
(2025) extends this pipeline with a lightweight agentic memory that caches recent observations
and reflections. It provides a minimal form of experience reuse without dedicated search or update
policies, forming a bridge between memory-free agents and adaptive memory systems.
Adaptive Agentic Memory Methods.
This group focuses on adaptive retrieval and self-evolving
memory. SelfRAG (Asai et al., 2024b) integrates dynamic retrieval and reflection to adaptively
ground reasoning in prior contexts. MemOS (Li et al., 2025), Mem0 (Chhikara et al., 2025), and
LangMem (LangChain contributors) implement structured, agent-level memory systems that support
read, write, and update operations. Within our unified interface, retrieval corresponds to the search
stage and updates correspond to evolve. These methods represent adaptive long-term agents capable
of continual refinement.
Memory-Based Agents for Procedural Memory.
Dynamic Cheatsheet (DC) (Suzgun et al., 2025)
and Agent Workflow Memory (AWM) (Wang et al., 2024) emphasize the reuse of procedural
knowledge, encoding “how-to” information rather than static facts. We evaluate two DC variants, DC-
RS (retrieval-based) and DC-Cu (curated), to analyze how workflow induction and update mechanisms
18


Evo-Memory: Benchmarking LLM Agent Test-time Learning with Self-Evolving Memory
Figure 5 | Memory pruning rates by dataset. Retained (blue) and pruned (coral) memory proportions
show varying selectivity across benchmarks.
influence stability and transfer. These methods test the potential of procedural memory as reusable
strategy repositories.
Proposed: Evolving Memory Framework.
ExpRecent maintains condensed episodic traces of
recent task trajectories, while our ExpRAG family integrates the principles of retrieval-augmented
reasoning with explicit test-time evolution. ReMem applies iterative reflection and synthesis to refine
memory embeddings over time. Together, these methods instantiate Evo-Memory’s design philosophy,
treating reasoning, acting, and memory refinement as interleaved processes that co-adapt during
deployment, enabling continual self-improvement and more human-like adaptation.
B. Experiments
We provide more experiments in the following.
B.1. Additional Experiments
We further validate our findings through extensive benchmarking across multiple model families
(Gemini-2.5-Flash-Lite, Claude-3.5-Haiku) and diverse datasets, as shown in Tables 4 and 5. The
performance trends remain consistent across all settings. On both multi-turn embodied reasoning tasks
(Alf World, BabyAI, PDDL, ScienceWorld) and single-turn reasoning tasks (AIME-24/25, GPQA, MMLU-
Pro, ToolBench), ReMem consistently outperforms conventional baselines and adaptive retrieval
methods across model backbones. These results confirm that the advantages of evolving-memory
architectures are model-agnostic, highlighting continual task-level reflection as a general mechanism
for improving adaptability in problem-solving.
19


Evo-Memory: Benchmarking LLM Agent Test-time Learning with Self-Evolving Memory
B.2. Additional Analysis of Memory Pruning
Figure 5 shows memory pruning rates across datasets, revealing varying selectivity in memory
retention. The pruning ratios differ substantially across benchmarks, which appears related to
task diversity and domain coverage. Datasets with broader domain coverage such as GPQA, which
encompasses diverse problem types across engineering, physics, and other domains, exhibit higher
pruning rates (36.8%), suggesting that more memories are deemed redundant across heterogeneous
tasks. In contrast, datasets with more concentrated problem types like AIME show lower pruning
rates (17.5% and 10.8% respectively), indicating that memories remain more relevant due to higher
task similarity. This pattern suggests that the pruning mechanism effectively identifies and discards
domain-irrelevant experiences, though the precise relationship between task diversity and memory
selectivity warrants further investigation.
B.3. Cumulative Accuracy Across Tasks and Models
Figure 6 shows cumulative accuracy over task sequences across four interactive multi-turn datasets.
The curves primarily compare ReMem with the History baseline, as individual trajectories are not
meaningful in isolation. Across all environments, ReMem exhibits faster adaptation and more stable
retention, demonstrating robustness under long task sequences.
Figure 7 presents cumulative accuracy curves comparing ReMem with the baseline across single-
turn reasoning benchmarks and model variants. As task instances accumulate, ReMem shows consis-
tent improvement on GPQA, ToolBench, and MMLU-PRO (Engineer) for both Gemini-2.5-Flash-Lite
and Claude-3.7-Sonnet. Similar to the multi-turn results, History performs comparably at the be-
ginning due to the cold-start phase, but ReMem quickly surpasses it as more tasks are processed,
indicating the cumulative advantage of continual task-level adaptation.
C. Potential Risks
Agent memory management relies on the outputs and judgments of large language models, and is
therefore subject to their reliability limitations. Imperfect or inconsistent LLM outputs may lead to
unreliable memory updates, which can affect subsequent retrieval and decision making. In addition,
memory-based agents may be vulnerable to attacks or poisoning, where adversarial or misleading
interactions introduce corrupted experiences into memory. Addressing reliability, robustness, and
security concerns in LLM-driven memory management remains an important direction for future
work.
20


Evo-Memory: Benchmarking LLM Agent Test-time Learning with Self-Evolving Memory
Figure 6 | Cumulative success rate across four interactive agent datasets, shown as rolling averages
over fixed task sequences (not learning curves).
Figure 7 | Cumulative accuracy comparison across model variants and benchmarks. ReMem (solid
blue) demonstrates consistent improvements over the History baseline (dashed red) across Gemini-2.5-
Flash-Lite and Claude-3.7-Sonnet models on GPQA, ToolBench, and MMLU-Pro (ENG) datasets. The
curves show learning trends as task instances accumulate, with ReMem achieving faster convergence
and higher final accuracy.
21


Evo-Memory: Benchmarking LLM Agent Test-time Learning with Self-Evolving Memory
D. Prompts
Memory Prompt Template for Multi-turn Dataset
=====================================
ENVIRONMENT INSTRUCTIONS
=====================================
[Detailed task environment description and rules]
Example: Go to kitchen, pick up apple, put it in bag, etc.
=====================================
EXAMPLE DEMONSTRATIONS
=====================================
[Static few-shot examples]
Example 1: Goal: ... | Action: ... | Observation: ...
Example 2: Goal: ... | Action: ... | Observation: ...
=====================================
RELEVANT EXPERIENCE FROM SIMILAR TASKS
=====================================
[Experience #1]
Goal: [similar goal]
Trajectory: [action sequence]
Correctness: [success/failure]
[Experience #2, #3, ...]
=====================================
YOUR CURRENT TASK
=====================================
Goal: [specific task goal]
Help: type ’check valid actions’ if action fails
Help: type ’inventory’ to check items
=====================================
RECENT HISTORY
=====================================
Observation:
[initial environment state]
Action:
[previous action]
Observation:
[result of previous action]
Action:
[previous action]
Observation:
[current state]
=====================================
OUTPUT FORMAT
=====================================
You MUST respond in EXACTLY ONE of these formats:
Format 1 - Prune experiences:
Think-Prune:
<IDs>
Remove unhelpful experiences from ’RELEVANT EXPERIENCE’ section (e.g., “1,3” or “2-4”)
Format 2 - Internal reasoning:
Think:
<your reasoning>
Free-form explanation of your next step
Format 3 - Execute action:
Action:
<exact command>
Must be valid command from ENVIRONMENT INSTRUCTIONS with exact names from RECENT HISTORY
22


Evo-Memory: Benchmarking LLM Agent Test-time Learning with Self-Evolving Memory
Memory Prompt Template for Single-turn Dataset
You are a helpful assistant with access to LOCAL EXPERIENCE MEMORY. Each memory may
contain past experience, rationales, domains, and skills. Below are some retrieved LOCAL
EXPERIENCE MEMORIES:
[Retrieved/synthesized memories]
Now solve the following problem.
Question: [Your question here]
Provide your output in the following format:
• Rationale: your short reasoning, may cite memory if useful
• Final Answer: your final answer
E. Ethical Considerations and Artifact Documentation
E.1. Cite Creators of Artifacts
All external artifacts used in this work are properly credited to their original publications and
repositories in Section 4.1.
E.2. Discuss the License for Artifacts
We comply with the licenses and terms of use of all artifacts employed in this study. The benchmark
datasets, such as MMLU-Pro, GPQA, Alf World, etc., are publicly available for research and evaluation
purposes under their respective licenses (typically Creative Commons or similar non-commercial
research licenses). Access to proprietary LLMs, such as Gemini and Claude, is conducted through
official APIs in accordance with the providers’ usage policies.
All newly introduced code, configurations, and evaluation pipelines for Evo-Memory will be
released under the permissive open-source license to support transparency and reproducibility upon
acceptance.
E.3. Artifact Use Consistent with Intended Purpose
We confirm that all datasets and models are used in accordance with their intended purposes. The
selected benchmarks are designed to evaluate reasoning, factual recall, tool use, and multi-turn
interaction, which directly aligns with our goal of assessing test-time learning and self-evolving
memory in LLM agents.
The evaluated LLMs are used solely for inference and controlled test-time interaction, without
modifying their pre-trained parameters. Memory modules operate externally and do not alter the
underlying model weights, ensuring consistency with the original model design.
E.4. Personally Identifying Information or Offensive Content
The datasets used in this work are established academic benchmarks widely adopted by the research
community. To the best of our knowledge, they do not contain personally identifying information.
While some datasets may reflect biases or problematic content inherited from web-scale sources,
23


Evo-Memory: Benchmarking LLM Agent Test-time Learning with Self-Evolving Memory
these issues are not unique to our work and are consistent with known limitations of the original
benchmarks.
We do not introduce new user-generated content or collect personal data as part of this study.
E.5. Documentation of Artifacts
We provide detailed documentation of all datasets, model backbones, and memory methods evaluated
in this work. Appendix A.4 and A.2 details the compared memory mechanisms and implementation
choices. Prompt templates and configuration files are included in Appendix D to facilitate full
reproducibility.
F. Statistics for Data
Dataset statistics are summarized below. Additional details are provided in Appendix A.1.
F.1. Single-Turn Reasoning Datasets
• MMLU-Pro: A multi-domain multiple-choice benchmark consisting of approximately 12K ques-
tions spanning 14 subject areas, including engineering, economics, philosophy, and natural
sciences. In our evaluation, we include major domains such as Engineering (969 questions,
aggregating subfields such as EE and ME), Economics (844 questions, covering both macro- and
micro-economics), and Philosophy (499 questions).
• GPQA-Diamond: 198 expert-curated, graduate-level multiple-choice questions designed to be
Google-proof, focusing on deep scientific reasoning.
• AIME-24 / AIME-25: 30 Olympiad-style mathematics problems per year, requiring exact-match
symbolic reasoning.
• ToolBench: 750 tool-use and API grounding tasks, evaluating the model’s ability to select and
invoke appropriate tools, measured by exact match and execution-based accuracy.
F.2. Multi-Turn Interactive Environments
• Alf World: 134 text-based embodied household tasks, requiring multi-step planning and inter-
action.
• BabyAI: 112 grid-based navigation and object manipulation tasks with compositional language
instructions.
• ScienceWorld: 90 interactive science tasks spanning physics, chemistry, and biology, requiring
long-horizon reasoning.
• PDDL: 60 symbolic planning problems expressed, evaluating goal-directed planning and state
transitions.
G. Computational Experiments
All experiments are fully reproducible, with implementation details provided in Appendix A and
prompts listed in Appendix D.
24


Evo-Memory: Benchmarking LLM Agent Test-time Learning with Self-Evolving Memory
G.1. Models and Budgets
We evaluate Evo-Memory on multiple LLM backbones with varying capacities, including Gemini-2.5
Flash, Flash-Lite, and Pro, as well as Claude 3.5 Haiku and Claude 3.7 Sonnet. All experiments are
conducted using fixed API budgets across methods to ensure fair comparison. The total API compute
cost was on the order of tens of thousands of US dollars.
G.2. Descriptive Statistics
Performance is reported using task-appropriate metrics, including exact match for single-turn rea-
soning tasks, success and progress rates for multi-turn environments, as well as cumulative accuracy
and robustness over task streams. Reported results are averaged across multiple task instances and
trajectories.
G.3. Parameters for Packages
All experiments are implemented using standard Python tooling. API-based access to LLMs is handled
through the official SDKs provided by model vendors. Additional utilities for retrieval, logging, and
evaluation are implemented using PyTorch 2.7.1 and Weights & Biases.
H. AI Assistants in Research or Writing
During the preparation of this paper, we made limited and controlled use of large language models
(LLMs), specifically ChatGPT, as an auxiliary writing aid. The LLM was used only for stylistic
refinement, including improvements in clarity, grammar, and readability of text originally drafted by
the authors. All scientific ideas, analyses, experiments, and conclusions were fully developed, written,
and verified by the authors. Thus, LLMs were employed solely as a language-editing tool, without
contributing to the intellectual or scientific content of the work.
I. Limitations
While Evo-Memory offers a comprehensive evaluation of self-evolving memory, several practical
constraints remain. Due to budget and API limits, we focus on a selected set of strong LLMs rather
than exhaustively covering all available models. Additional evaluations on open-weight or multilingual
models could further validate the generality of our findings. Moreover, our benchmark primarily
emphasizes textual and goal-oriented tasks; extending it to richer multimodal or real-world envi-
ronments would provide a more complete picture of continual memory evolution. Despite these
limitations, the current study already spans diverse domains, tasks, and architectures, offering a solid
foundation for future extensions.
25


Evo-Memory: Benchmarking LLM Agent Test-time Learning with Self-Evolving Memory
LLM Backbone
Method
Alf World
BabyAI
PDDL
ScienceWorld
Avg.
S
P
S
P
S
P
S
P
S
P
Gemini 2.5 Flash
Baseline
0.12
0.34
0.61
0.71
0.12
0.20
0.24
0.59
0.27
0.46
History
0.28
0.60
0.52
0.64
0.08
0.15
0.31
0.71
0.30
0.53
ReAct
0.24
0.56
0.48
0.63
0.22
0.33
0.34
0.71
0.32
0.56
Amem
0.25
0.59
0.53
0.64
0.10
0.16
0.36
0.74
0.31
0.53
SelfRAG
0.25
0.59
0.52
0.65
0.08
0.16
0.34
0.74
0.30
0.54
Mem0
0.27
0.61
0.54
0.66
0.10
0.19
0.32
0.70
0.31
0.54
DC-Cu
0.25
0.59
0.53
0.64
0.08
0.17
0.29
0.71
0.29
0.53
DC-RS
0.27
0.60
0.53
0.66
0.07
0.15
0.33
0.73
0.30
0.54
AWM
0.26
0.59
0.52
0.64
0.08
0.16
0.33
0.73
0.30
0.53
ExpRecent
0.37
0.65
0.53
0.64
0.13
0.22
0.53
0.83
0.39
0.59
ExpRAG
0.59
0.79
0.56
0.65
0.17
0.27
0.53
0.81
0.46
0.63
ReMem
0.66
0.81
0.53
0.61
0.22
0.33
0.58
0.81
0.50
0.64
Gemini 2.5 Pro
Baseline
0.04
0.39
0.37
0.47
0.20
0.33
0.22
0.60
0.21
0.45
History
0.19
0.52
0.40
0.48
0.25
0.38
0.60
0.84
0.36
0.56
ReAct
0.02
0.26
0.43
0.55
0.13
0.22
0.30
0.68
0.22
0.43
Amem
0.16
0.50
0.42
0.49
0.23
0.38
0.59
0.85
0.35
0.56
SelfRAG
0.16
0.49
0.43
0.50
0.22
0.35
0.57
0.84
0.34
0.55
Mem0
0.16
0.49
0.41
0.49
0.22
0.37
0.53
0.81
0.33
0.54
DC-Cu
0.17
0.50
0.40
0.47
0.28
0.40
0.53
0.82
0.35
0.55
DC-RS
0.18
0.51
0.42
0.50
0.27
0.40
0.59
0.85
0.37
0.57
AWM
0.20
0.52
0.38
0.46
0.20
0.37
0.57
0.83
0.34
0.54
ExpRecent
0.36
0.61
0.54
0.64
0.35
0.47
0.69
0.89
0.49
0.64
ExpRAG
0.38
0.64
0.46
0.53
0.28
0.43
0.61
0.84
0.43
0.61
ReMem
0.51
0.70
0.56
0.64
0.25
0.38
0.66
0.86
0.50
0.65
Claude 3.7 Sonnet
Baseline
0.18
0.49
0.51
0.66
0.17
0.39
0.10
0.53
0.24
0.52
History
0.50
0.73
0.48
0.66
0.65
0.85
0.32
0.74
0.49
0.74
ReAct
0.51
0.75
0.57
0.72
0.75
0.91
0.44
0.77
0.57
0.79
Amem
0.48
0.73
0.46
0.64
0.62
0.84
0.33
0.73
0.47
0.73
SelfRAG
0.52
0.75
0.46
0.64
0.65
0.84
0.31
0.74
0.49
0.74
Mem0
0.51
0.74
0.48
0.66
0.65
0.84
0.37
0.76
0.50
0.75
DC-Cu
0.50
0.74
0.50
0.67
0.62
0.84
0.33
0.75
0.49
0.75
DC-RS
0.50
0.74
0.52
0.68
0.62
0.84
0.34
0.74
0.50
0.75
AWM
0.49
0.73
0.53
0.68
0.60
0.82
0.34
0.74
0.49
0.74
ExpRecent
0.66
0.83
0.63
0.73
0.53
0.76
0.49
0.82
0.58
0.79
ExpRAG
0.74
0.89
0.62
0.72
0.72
0.89
0.46
0.76
0.63
0.82
ReMem
0.92
0.96
0.73
0.83
0.83
0.95
0.62
0.89
0.78
0.91
Claude 3.5 Haiku
Baseline
0.11
0.33
0.38
0.52
0.15
0.32
0.08
0.37
0.18
0.39
History
0.28
0.58
0.38
0.57
0.18
0.38
0.12
0.49
0.24
0.51
ReAct
0.24
0.58
0.35
0.52
0.32
0.53
0.16
0.55
0.27
0.55
Amem
0.24
0.55
0.37
0.58
0.17
0.35
0.12
0.45
0.23
0.48
SelfRAG
0.26
0.58
0.38
0.59
0.22
0.37
0.14
0.49
0.25
0.51
Mem0
0.27
0.56
0.37
0.57
0.17
0.37
0.08
0.45
0.22
0.49
DC-Cu
0.24
0.55
0.37
0.58
0.17
0.37
0.12
0.45
0.23
0.49
DC-RS
0.24
0.55
0.37
0.58
0.17
0.37
0.12
0.45
0.23
0.49
AWM
0.24
0.55
0.37
0.58
0.17
0.37
0.12
0.45
0.23
0.49
ExpRecent
0.48
0.65
0.40
0.57
0.15
0.32
0.32
0.64
0.34
0.55
ExpRAG
0.65
0.74
0.54
0.64
0.43
0.61
0.42
0.68
0.51
0.67
ReMem
0.69
0.80
0.49
0.60
0.43
0.61
0.44
0.75
0.51
0.69
Table 4 | Cross-environment results across four embodied reasoning benchmarks (Alf World, BabyAI,
PDDL, ScienceWorld). Each dataset reports success (S) and progress (P) rates. Bold indicates the
best (including ties) per column. The last two columns show averaged S and P across datasets.
26


Evo-Memory: Benchmarking LLM Agent Test-time Learning with Self-Evolving Memory
LLM Backbone
Method
Exact Match ↑
API / Acc. ↑
AIME24
AIME25
GPQA
MMLU-Pro (Eco.)
MMLU-Pro (Eng.)
MMLU-Pro (Philo.)
ToolBench
Avg. ↑
Claude 3.5
Baseline
—
—
0.36
0.68
0.42
0.55
0.81/0.64
0.38
History
—
—
0.37
0.70
0.43
0.55
0.81/0.63
0.38
ReAct
—
—
0.35
0.69
0.43
0.54
0.81/0.64
0.38
Amem
—
—
0.34
0.69
0.43
0.53
0.82/0.63
0.37
SelfRAG
—
—
0.36
0.70
0.44
0.56
0.83/0.65
0.39
MemOS
—
—
0.37
0.70
0.42
0.55
0.81/0.64
0.38
Mem0
—
—
0.36
0.70
0.42
0.55
0.82/0.64
0.38
LangMem
—
—
0.51
0.78
0.46
0.61
0.81/0.63
0.43
DC-RS
—
—
0.36
0.68
0.41
0.56
0.82/0.64
0.38
AWM
—
—
0.33
0.67
0.42
0.53
0.81/0.63
0.37
DC-Cu
—
—
0.33
0.65
0.39
0.54
0.82/0.63
0.36
ExpRecent
—
—
0.42
0.69
0.46
0.59
0.85/0.63
0.40
ExpRAG
—
—
0.40
0.73
0.49
0.61
0.87/0.67
0.41
ReMem
—
—
0.39
0.71
0.47
0.62
0.87/0.68
0.41
Claude 3.7 Sonnet
Baseline
0.17
0.13
0.55
0.84
0.63
0.78
0.76/0.62
0.54
History
0.13
0.23
0.56
0.85
0.64
0.78
0.76/0.61
0.55
ReAct
0.17
0.10
0.57
0.84
0.63
0.76
0.76/0.61
0.54
Amem
0.27
0.17
0.54
0.83
0.63
0.79
0.77/0.63
0.56
SelfRAG
0.20
0.10
0.58
0.84
0.65
0.77
0.77/0.63
0.55
MemOS
0.17
0.20
0.55
0.84
0.64
0.76
0.76/0.62
0.55
Mem0
0.20
0.13
0.58
0.84
0.62
0.77
0.76/0.61
0.55
LangMem
0.10
0.13
0.53
0.77
0.56
0.66
0.77/0.63
0.49
DC-RS
0.20
0.20
0.62
0.79
0.52
0.60
0.77/0.62
0.52
AWM
0.03
0.03
0.53
0.80
0.56
0.72
0.76/0.62
0.48
DC-Cu
0.17
0.23
0.57
0.79
0.52
0.65
0.77/0.62
0.52
ExpRecent
0.13
0.20
0.61
0.86
0.63
0.78
0.82/0.66
0.56
ExpRAG
0.17
0.17
0.70
0.85
0.67
0.80
0.88/0.72
0.59
ReMem
0.13
0.13
0.67
0.86
0.65
0.80
0.87/0.71
0.58
Gemini 2.5 Flash
Baseline
0.47
0.47
0.48
0.83
0.46
0.75
0.71/0.61
0.59
History
0.60
0.47
0.43
0.84
0.42
0.78
0.31/0.26
0.55
ReAct
0.30
0.27
0.05
0.64
0.16
0.54
0.64/0.57
0.37
Amem
0.70
0.57
0.52
0.83
0.42
0.72
0.72/0.60
0.63
SelfRAG
0.50
0.47
0.46
0.83
0.45
0.75
0.72/0.61
0.59
MemOS
0.47
0.47
0.50
0.82
0.46
0.75
0.71/0.61
0.59
Mem0
0.50
0.47
0.45
0.83
0.46
0.74
0.71/0.61
0.59
LangMem
0.43
0.50
0.53
0.79
0.39
0.71
0.68/0.57
0.57
DC-RS
0.53
0.37
0.48
0.80
0.42
0.69
0.68/0.57
0.56
DC-Cu
0.60
0.40
0.48
0.79
0.44
0.69
0.70/0.59
0.58
AWM
0.50
0.37
0.49
0.79
0.43
0.72
0.71/0.59
0.56
ExpRecent
0.47
0.47
0.42
0.83
0.39
0.75
0.78/0.66
0.58
ExpRAG
0.43
0.47
0.42
0.83
0.43
0.78
0.87/0.73
0.60
ReMem
0.60
0.53
0.51
0.85
0.46
0.79
0.85/0.71
0.65
Gemini 2.5 Flash-Lite
Baseline
0.53
0.43
0.37
0.73
0.34
0.60
0.78/0.61
0.58
History
0.40
0.33
0.31
0.74
0.30
0.59
0.58/0.47
0.49
ReAct
0.53
0.33
0.34
0.61
0.20
0.48
0.73/0.56
0.50
Amem
0.40
0.33
0.33
0.72
0.34
0.59
0.77/0.61
0.54
SelfRAG
0.57
0.37
0.37
0.73
0.32
0.62
0.81/0.63
0.57
MemOS
0.53
0.43
0.37
0.73
0.34
0.60
0.78/0.61
0.58
Mem0
0.53
0.43
0.37
0.73
0.34
0.60
0.78/0.61
0.58
LangMem
0.40
0.37
0.48
0.59
0.24
0.56
0.33/0.26
0.43
DC-RS
0.53
0.43
0.34
0.59
0.18
0.36
0.73/0.56
0.49
AWM
0.03
0.03
0.35
0.61
0.20
0.48
0.77/0.60
0.44
DC-Cu
0.53
0.43
0.33
0.55
0.16
0.32
0.71/0.56
0.47
ExpRecent
0.57
0.33
0.35
0.76
0.29
0.62
0.82/0.65
0.58
ExpRAG
0.47
0.37
0.38
0.79
0.32
0.66
0.87/0.68
0.61
ReMem
0.57
0.33
0.38
0.77
0.34
0.65
0.86/0.67
0.61
Table 5 | Cross-dataset results of diverse memory architectures across models. Categories are separated
by horizontal rules; results (Exact Match↑and API/Acc↑) compare zero-shot, agentic, adaptive,
procedural, and proposed memory methods. Dashes (—) indicate methods with poor or unreliable
performance, which are therefore omitted.
27
