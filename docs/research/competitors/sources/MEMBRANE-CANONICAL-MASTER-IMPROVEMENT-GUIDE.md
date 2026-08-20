# Membrane — Canonical Master Improvement & Implementation Guide

**Status:** proposed canonical implementation authority  
**Repository:** `Orthic-Labs/Membrane`  
**Date:** 2026-08-18  
**Evidence basis:** current Membrane architecture plus the four competitor-analysis passes and their consolidated implementation guides covering the 60-repository competitor corpus.  
**Purpose:** replace the accumulated competitor recommendations with one reasoned, deduplicated, implementation-oriented improvement program.

---

# 0. Executive decision

Membrane does **not** need a new architecture.

It already has the difficult and differentiated part of the system:

- `ScopeGrant`;
- `ContextCandidateSet`;
- `ContextPacket`;
- `ContextReceipt`;
- `KnowledgeEmission`;
- Push / Pull / Persist under one context economy;
- cross-provider federation;
- explicit scope;
- authority and freshness;
- global context-budget reconciliation;
- multiple delivery lanes;
- durable local persistence;
- lexical and semantic retrieval;
- temporal facts;
- supersession;
- compression receipts;
- resolver-backed source recovery;
- resident services;
- deterministic degradation;
- separation between Membrane, Cortex, Blueprint, Audit, Architect and the surrounding host/runtime.

The competitor research overwhelmingly argues for **finishing and connecting these primitives**, not replacing them.

The target is:

> **Membrane should become the canonical context-control system that governs how information is admitted, represented, stored, indexed, related, retrieved, transformed, delivered, remembered, forgotten and verified — while producing evidence for every material decision.**

The important refinement for Membrane specifically is that **memory is only one class of context**.

Membrane's governed knowledge universe includes:

```text
Documents / Markdown spine
Memories
Decisions
Taste / preferences
Gotchas / insights
Procedures / lessons
Episodes / sessions
Temporal facts
Artifacts
Rules / policy
Audit evidence
Live files / Git state
Code semantics supplied by Blueprint
```

These should participate in a common context economy without being flattened into one indistinguishable blob.

---

# 1. Product definition

Membrane is best defined as a:

> **local-first, evidence-aware context control plane and context compiler.**

It controls four related problems.

## 1.1 Knowledge ingestion

What information is allowed to enter the system?

```text
source
  ↓
scope / security validation
  ↓
classification
  ↓
identity + evidence
  ↓
duplicate/conflict detection
  ↓
admission decision
  ↓
canonical knowledge
```

## 1.2 Knowledge maintenance

What happens after information has been stored?

```text
active
  ↓
reinforced / unchanged / contradicted / superseded
  ↓
warm / cold
  ↓
archive / expire / quarantine
```

History remains available even when an item stops participating in current retrieval.

## 1.3 Knowledge retrieval

What information should be considered for a task?

```text
request
  ↓
candidate channels
  ↓
scope / authority / freshness
  ↓
retrieval + relations
  ↓
fusion
  ↓
diversity
  ↓
budget allocation
```

## 1.4 Context compilation

What should actually reach the model?

```text
eligible evidence
  ↓
representation selection
  ↓
externalization / compression
  ↓
critical-evidence verification
  ↓
global budget
  ↓
ContextPacket
  +
ContextReceipt
```

A memory system answers:

> "What should I remember?"

Membrane answers:

> **"Given everything I know and can currently access, what evidence should this task receive, in what representation, under what authority and budget, and why?"**

---

# 2. Architectural invariants

Every improvement below should preserve these constraints.

## 2.1 Keep the existing five public protocol shapes

Do not create a second public protocol family for every new internal feature.

Canonical public spine:

```text
ScopeGrant
ContextCandidateSet
ContextPacket
ContextReceipt
KnowledgeEmission
```

Add public fields only when a real consumer requires them.

## 2.2 The planner remains sovereign

Providers generate candidates.

They do **not** decide final context policy.

The canonical planner owns:

```text
eligibility
scope
authorization
authority
freshness
cross-source fusion
budget admission
delivery mode
omissions
publication
receipt reconciliation
```

No provider should independently decide:

> "This candidate deserves 2,000 final tokens."

Providers describe evidence.

Membrane decides attention.

## 2.3 Never compare arbitrary raw scores globally

These are not equivalent quantities:

```text
Cortex vector cosine
Blueprint graph relevance
Git freshness
rule priority
BM25 score
feedback effectiveness
temporal relevance
```

A magic formula such as:

```text
0.40 semantic
+ 0.20 graph
+ 0.15 freshness
+ 0.15 trust
+ 0.10 feedback
```

creates false calibration.

Instead:

```text
1. hard eligibility
2. authority
3. freshness
4. provider/channel-local ranking
5. rank-level fusion
6. bounded utility adjustment
7. diversity
8. global context admission
```

## 2.4 SQLite/Cortex remains canonical durable truth

Do not create competing writable knowledge stores.

Recommended ownership:

```text
SQLite / Cortex
    canonical structured knowledge

FTS5
    rebuildable lexical projection

Vector index
    rebuildable semantic projection

Relation graph
    rebuildable/derived relationship projection where possible

Markdown
    source evidence OR exported human-readable representation

Git
    source/freshness evidence

Artifact store
    immutable/recoverable raw material
```

This is especially important for the document spine.

## 2.5 Blueprint owns code semantics

Do not duplicate Blueprint inside Membrane.

Blueprint should own:

```text
parsing
ASTs
symbols
references
imports
calls
types
code relationships
entry points
blast radius
rename/move tracking
code snapshots
code diffs
failure-signal resolution
coverage/confidence
```

Membrane consumes the evidence.

Membrane's job is:

```text
Is this evidence current?
Is it authorized?
How authoritative is it?
How does it relate to the request?
How much context should it receive?
Should it be delivered directly or by resolver?
```

## 2.6 Keep Application / Control / Data separation

Preserve the existing planes:

```text
Application
    CLI / MCP / HTTP / request handling

Control
    supervision / leases / lifecycle / health

Data
    SQLite / indexes / receipts / durable storage
```

Do not allow convenience features to collapse these boundaries.

---

# 3. Target architecture

```text
                         Host / MCP / Hooks
                                │
                                ▼
                    ScopeGrant + Request Identity
                                │
                                ▼
┌──────────────────────────────────────────────────────────────────┐
│                     MEMBRANE PLANNER                             │
│                                                                  │
│ eligibility                                                      │
│      ↓                                                           │
│ authority / freshness                                            │
│      ↓                                                           │
│ candidate fusion                                                 │
│      ↓                                                           │
│ context economy / global budget                                  │
│      ↓                                                           │
│ representation + delivery                                        │
│      ↓                                                           │
│ omissions + receipt                                              │
└──────────────────────────────────────────────────────────────────┘
          │              │              │              │
          ▼              ▼              ▼              ▼
       Blueprint           Cortex       Live / Git     Docs / Artifacts
   code semantics     knowledge     current facts   indexed evidence
                         │
                         ├── exact / anchor
                         ├── FTS5 / BM25
                         ├── vector
                         ├── temporal
                         ├── relation
                         └── active / episodic
          │
          └────────────────────┬───────────────────────────────────
                               ▼
                      eligible candidates
                               │
                               ▼
                    two-phase context fill
                               │
                               ▼
             native / rendered / ArtifactRef /
                       metadata-only
                               │
                               ▼
                ContextPacket + ContextReceipt
                               │
                               ▼
                            model
                               │
                               ▼
              used / ignored / contradicted /
                        task outcome
                               │
                               ▼
                  lifecycle signal ledger
```

---

# 4. Canonical knowledge model

This should become the substrate beneath documents, memories, taste, gotchas and sessions.

## 4.1 Separate identity dimensions

Do not use one ID for every concept.

Use:

```text
logical_id
    stable identity of the conceptual item

content_sha256
    exact immutable content/version identity

event_id
    identity of lifecycle/write/change event

artifact_id
    immutable raw artifact identity

evidence_ref
    reference supporting the claim

source_ref
    original source identity/resolver
```

---

# 5. Knowledge taxonomy

The source material proposes these internal knowledge families:

```rust
Observation
Episode
SemanticFact
Procedure
Preference
EntitySummary
EvolvingBelief
ArtifactReference
```

and lifecycle states:

```rust
Active
Warm
Cold
Archived
Superseded
Expired
Quarantined
Tombstoned
```

Those are useful because they separate **what something is** from **whether it is currently active**.

For Membrane's actual product vocabulary, add another orthogonal dimension: `KnowledgeKind`.

Example:

```text
family: SemanticFact
kind: architecture_decision

family: Preference
kind: taste

family: Procedure
kind: gotcha

family: Episode
kind: session

family: SemanticFact
kind: project_constraint

family: ArtifactReference
kind: markdown_document
```

Potential `KnowledgeKind` values:

```text
document
section
decision
constraint
memory
taste
preference
gotcha
insight
lesson
procedure
failure
success
entity
relationship
session
task
artifact
rule_reference
code_claim
```

Do not create separate persistence systems for every category.

They should share identity, evidence, temporal semantics and lifecycle machinery while retaining category-specific policy.

---

# 6. Improvement 1 — Establish one canonical plan

**Priority:** P0  
**Dependency:** none

Create one implementation authority:

```text
docs/MEMBRANE-IMPLEMENTATION-GUIDE.md
```

Mark older plans superseded.

Required outcome:

```text
What are we building?
Why?
In what order?
What is done?
What is experimental?
What is rejected?
```

---

# 7. Improvement 2 — Freeze an evaluation baseline

**Priority:** P0  
**Dependency:** Improvement 1

Do this before changing ranking, memory policy, graph expansion or compression.

Create canonical cases covering:

### Retrieval

- exact identifier;
- conceptual retrieval;
- lexical-only retrieval;
- semantic-only retrieval;
- multi-source overlap;
- no relevant evidence.

### Freshness

- current file vs stale memory;
- superseded decision;
- changed source;
- missing source;
- dirty worktree.

### Documents

- exact Markdown heading;
- relevant section inside large document;
- superseded document;
- cross-document reference;
- updated document with unchanged unrelated sections.

### Taste/preferences

- active explicit preference;
- outdated preference;
- conflicting preference;
- scope-specific preference.

### Gotchas/insights

- known failure applicable to current task;
- unrelated historical failure;
- gotcha linked to obsolete code.

### Security

- cross-scope data;
- secret-bearing source;
- prompt injection stored as memory;
- expired grant.

### Context pressure

- oversized tool result;
- huge document;
- competing providers;
- strict token ceiling.

### Failure

- Blueprint unavailable;
- Cortex unavailable;
- resolver unavailable;
- provider timeout;
- partial provider completion.

Metrics should remain separate:

```text
Recall@K
Precision@K
MRR
nDCG
required-evidence recall
forbidden-evidence admission
stale-evidence admission
contradiction miss rate
source-resolution success
receipt completeness
token consumption
bytes avoided
p50/p95/p99
RSS
task outcome
```

---

# 8. Improvement 3 — Add a canonical knowledge envelope

**Priority:** P1  
**Dependencies:** evaluation baseline

Every durable item should be representable as:

```text
KnowledgeRecord
├── logical identity
├── exact content identity
├── family
├── kind
├── lifecycle state
├── scope
├── authority
├── veracity/confidence
├── influence class
├── sensitivity class
├── observed time
├── valid time
├── expiry
├── provenance/evidence
├── supersession
├── derivation lineage
└── mutable retrieval signals
```

Critically, **mutable ranking signals must not mutate canonical content identity**.

---

# 9. Improvement 4 — Make the document spine first-class

**Priority:** P1  
**Dependencies:** canonical identity

The document spine should become one of Membrane's principal knowledge sources.

It should not merely mean:

```text
*.md
→ chunk
→ embed
→ search
```

Instead:

```text
Markdown repository
      │
      ▼
Document identity
      │
      ├── path
      ├── content hash
      ├── parser version
      ├── document type
      ├── authority
      ├── scope
      └── revision
      │
      ▼
Document structure
      │
      ├── headings
      ├── sections
      ├── anchors
      ├── references
      ├── frontmatter
      ├── entities
      ├── code/file references
      └── source ranges
      │
      ▼
Knowledge projection
      │
      ├── decisions
      ├── constraints
      ├── procedures
      ├── taste
      ├── gotchas
      ├── insights
      └── evidence
```

## 9.1 Preserve source fidelity

The Markdown document remains the evidence.

Derived records should point back to exact source ranges.

## 9.2 Index hierarchically

Store/index at multiple levels:

```text
document
section
subsection
atomic claim
explicit named item
```

## 9.3 Content-hash incrementality

Use:

```text
file hash
section hash
parser/extractor version
```

to determine what actually requires recomputation.

---

# 10. Improvement 5 — Add write validation

**Priority:** P0/P1  
**Dependency:** canonical knowledge envelope

Every persistence candidate should pass:

```text
schema validity
scope validity
source/evidence validity
security classification
identity calculation
duplicate check
conflict check
temporal validity
admission policy
```

Only then can it become canonical.

---

# 11. Improvement 6 — Make "no-op" a first-class result

**Priority:** P1

The system must be allowed to conclude:

> **Nothing worth remembering occurred.**

Canonical write outcomes should include:

```text
retained
updated
superseded
merged
quarantined
proposal
rejected
no_op
```

`no_op` is not failure.

It is successful filtering.

---

# 12. Improvement 7 — Build explicit admission policy

**Priority:** P1

Before something becomes durable knowledge, ask:

```text
Is it novel?
Is it useful?
Is it durable?
Is there evidence?
Is it current?
Is it scoped correctly?
Is it merely conversational?
Is it redundant?
Is it an instruction or only descriptive text?
```

---

# 13. Improvement 8 — Add explicit conflict semantics

**Priority:** P1/P2

Handle:

```text
exact duplicate
refinement
supersession
temporal change
scope difference
entity mismatch
true contradiction
unresolved ambiguity
```

Rules:

```text
exact duplicate
    → no-op / reinforce

new authoritative version
    → supersede

simultaneously incompatible evidence
    → preserve both + conflict

weak derived claim vs direct evidence
    → quarantine / reduce influence

uncertain identity
    → keep separate
```

---

# 14. Improvement 9 — Complete temporal semantics

**Priority:** P2

Each relevant record may carry:

```text
observed_at
valid_from
valid_until
expires_at
supersedes
```

Important distinction:

```text
observed_at
    when Membrane learned it

valid_from / valid_until
    when it was true

expires_at
    when Membrane should stop treating it as active

transaction/event time
    when the database changed
```

Expiry must happen **before scoring**, not as a small ranking penalty.

---

# 15. Improvement 10 — Implement lifecycle/forgetting properly

**Priority:** P2

Recommended starting behavior:

```text
working
    hard capacity + task/session expiry

episode
    age decay
    successful reuse increases stability

semantic fact
    slow decay
    supersession/expiry dominates

procedure/gotcha
    failures and source drift matter more than wall clock

preference/taste
    explicit pinned/current preference does not decay simply because time passed

derived summary/evolving belief
    expire aggressively when supporting evidence changes

artifact
    retention/availability policy rather than relevance decay
```

Add hysteresis:

```text
promote cold → warm at >= A
demote warm → cold at < B

where B < A
```

---

# 16. Improvement 11 — Make taste a governed knowledge class

**Priority:** P2

Taste should not be an unstructured bag of sentences.

Represent it as preference knowledge with evidence and scope.

Example:

```text
family: Preference
kind: taste

subject:
    UI

claim:
    prefers dense information architecture over oversized cards

scope:
    design

authority:
    explicit_user

confidence:
    high

observed_at:
    ...

supersedes:
    ...
```

Taste may be:

```text
global
domain-specific
project-specific
task-specific
temporary
```

---

# 17. Improvement 12 — Make gotchas/insights first-class procedural knowledge

**Priority:** P2

A gotcha usually has the shape:

```text
context
trigger
failure/risk
lesson
recommended behavior
evidence
applicability
```

Useful fields:

```text
trigger
applies_to
avoid
prefer
severity
confidence
source
verification
last_applicable
source_drift_state
```

This allows retrieval to surface gotchas when their trigger applies, rather than simply because their prose is semantically similar.

---

# 18. Improvement 13 — Production-grade FTS5/BM25

**Priority:** P1/P2

Target:

```text
exact / anchor
FTS5 / BM25
vector
temporal
relations
active/session
```

FTS requirements:

```text
Unicode-safe normalization
phrase matching
identifier awareness
path awareness
field weighting
scope filters
authority filters
deterministic fallback
```

No Elasticsearch.

No remote search service.

No mandatory network dependency.

---

# 19. Improvement 14 — Build retrieval as explicit channels

**Priority:** P2

Canonical Cortex retrieval channels:

1. exact/anchor/entity/path;
2. FTS5/BM25;
3. semantic vector;
4. temporal;
5. relation/entity;
6. active/working/session.

Each candidate should record:

```text
channels that retrieved it
rank in each channel
authority class
freshness class
policy class
runtime modifiers
final decision
```

---

# 20. Improvement 15 — Add retrieval explanation traces

**Priority:** P2

For every candidate, be able to answer:

```text
Why was it found?
Why was it eligible?
Why did it rank here?
Why did it enter context?
Why was another candidate omitted?
```

---

# 21. Improvement 16 — Implement two-phase budget allocation

**Priority:** P1/P2  
**Confidence:** highest-confidence algorithmic recommendation in the corpus

Instead of:

```text
sort candidates
take until budget full
```

do:

## Phase A — breadth floor

Give important candidates their minimum useful representation.

## Phase B — depth upgrade

Spend remaining budget increasing detail for the highest-value candidates.

This prevents one giant document or tool result from starving every other evidence class.

---

# 22. Improvement 17 — Make Push artifact-first and reversible

**Priority:** P1/P2

Canonical flow:

```text
large content
    ↓
durable hash-addressed raw artifact
    ↓
structure-aware reduction
    ↓
smallest useful representation
    ↓
ArtifactRef back to exact original
```

Use for:

```text
large tool output
logs
documents
tables
screenshots
OCR
transcripts
reports
generated artifacts
```

---

# 23. Improvement 18 — Add query-critical restoration

**Priority:** P2

Protected evidence includes:

```text
identifiers
errors
status codes
test names
citations
source ranges
query entities
authority-bearing rules
explicitly requested details
```

If compression loses them:

```text
resolver → exact source spans → restore
```

Failure semantics should be conservative:

```text
artifact write failure
    → keep raw

reducer failure
    → use less compressed representation

verifier uncertainty
    → restore exact spans

resolver failure
    → explicit artifact_unavailable omission
```

---

# 24. Improvement 19 — Source-anchor and drift verification

**Priority:** P2

Every evidence-linked item should eventually resolve to:

```text
current
moved
drifted
ambiguous
missing
unsupported
```

Blueprint supplies move/rename-stable code identity.

Membrane consumes it.

Do not infer competing symbol identities from raw paths.

---

# 25. Improvement 20 — Strengthen the Blueprint bridge

**Priority:** P2

Membrane should request compact semantic operations from Blueprint:

```text
symbol_lookup
references
related_context
impact
failure_signal
entry_points
change_context
claim_evidence
```

A Blueprint evidence response should provide enough information for Membrane to reason about:

```text
stable symbol ID
source path/range
source hash
revision
dirty overlay
relationship
confidence
coverage
generation
verification status
resolver
```

If Blueprint fails, only the Blueprint evidence lane should degrade.

Membrane continues functioning.

---

# 26. Improvement 21 — Add a narrow temporal relation layer

**Priority:** P2/P3

Useful relation types include:

```text
derived_from
supports
contradicts
supersedes
applies_to
depends_on
caused_by
related_to
same_as
part_of
mentions
produced_by
```

Relations must preserve evidence.

A relation should not become canonical merely because an embedding model thought two sentences looked similar.

---

# 27. Improvement 22 — Entity and alias resolution

**Priority:** P3

Keep alias relations first.

Do not destructively merge entities until identity is sufficiently proven.

---

# 28. Improvement 23 — Session packets

**Priority:** P2

At task/session close, create an episodic packet containing:

```text
session/task ID
repo/worktree/revision
goal
material decisions
open work
failed approaches
verification/results
important identifiers
artifact refs
contradictions/uncertainty
evidence refs
```

The packet itself is episodic memory.

Do **not** automatically promote every sentence into semantic memory.

---

# 29. Improvement 24 — Retroactive session mining

**Priority:** P3

Recommended flow:

```text
host transcript/session store
    ↓
offline discovery
    ↓
candidate session packet
    ↓
proposal
    ↓
normal admission pipeline
```

Not:

```text
intercept every token live
```

---

# 30. Improvement 25 — Controlled promotion

**Priority:** P2/P3

Promotion candidates:

```text
stable repeated preference
verified decision
procedure with successful outcome
authoritative current fact
repeated confirmed gotcha
```

---

# 31. Improvement 26 — Durable job/run model

**Priority:** P2/P3

Introduce:

```text
Job
Run
Checkpoint
RunReceipt
```

A run needs:

```text
job_id
run_id
kind
status
started_at
finished_at
checkpoint
progress
items_scanned
items_changed
error
cancellation
receipt
```

States:

```text
queued
running
completed
failed
cancelled
interrupted
```

---

# 32. Improvement 27 — Background maintenance

**Priority:** P3

Bounded scheduled jobs:

```text
decay/state pass
duplicate scan
conflict scan
entity alias proposal
derived-summary staleness scan
artifact integrity scan
source-anchor verification
consolidation proposal
backup
health checks
```

Each job must have:

```text
row cap
time cap
CPU cap
checkpoint/resume
cancellation
lock/lease
typed receipt
```

Background maintenance must never block the prompt path.

---

# 33. Improvement 28 — Reversible curation/consolidation

**Priority:** P3

A consolidation should produce:

```text
new derived record
parent IDs
source hashes
algorithm/model version
prompt version if applicable
confidence
timestamp
rollback path
```

Never destroy the only source records.

---

# 34. Improvement 29 — Separate evidence from influence

**Priority:** P1/P2  
**Security-critical**

Every knowledge record should distinguish:

```text
evidence authority
```

from:

```text
instructional influence
```

Possible classes:

```text
descriptive_only
advisory
user_preference
project_policy
system_policy
untrusted
quarantined
```

---

# 35. Improvement 30 — DLP, secrets and sensitive data

**Priority:** P2

Before persistence:

```text
secret scan
PII classification where relevant
scope classification
redaction policy
influence classification
```

Before retrieval/export:

```text
current grant
current policy
scope
sensitivity
```

must be rechecked.

---

# 36. Improvement 31 — Erasure semantics

**Priority:** P3

Deletion must remove sensitive data from all active projections:

```text
canonical active storage
FTS
vectors
relation indexes
artifact references
exports
caches
```

while maintaining the minimum tombstone/history information required to prevent accidental resurrection.

Backup/restore must respect erasure.

---

# 37. Improvement 32 — Complete the feedback loop

**Priority:** P2/P3

Bind downstream signals to delivered candidate IDs:

```text
delivered
used
ignored
contradicted
successful
failed
unknown
```

Do not infer "used" merely because context was delivered.

Unknown remains unknown.

Feedback may affect retrieval pressure.

It must **never alter authority**.

---

# 38. Improvement 33 — Mutable signal sidecar

**Priority:** P2

Maintain mutable retrieval/lifecycle signals separately from canonical records:

```text
access_count
last_access
successful_use_count
failed_use_count
effectiveness
hotness
decay_state
last_verified
contradiction_count
```

Include scoring algorithm/version epoch.

---

# 39. Improvement 34 — Observability as part of retrieval

**Priority:** P1/P2

For every recall record:

```text
query class
channel latency
candidate counts
eligibility drops
channel ranks
fused rank
selected reason
dropped reason
token cost
byte cost
artifact savings
provider status
feedback
```

---

# 40. Improvement 35 — Human-readable knowledge surface

**Priority:** P3/P4

Expose, preferably through Hub:

```text
Knowledge
├── Documents
├── Decisions
├── Memories
├── Taste
├── Gotchas
├── Procedures
├── Sessions
├── Entities
├── Conflicts
├── Archived
└── Quarantined
```

For every record show:

```text
content
kind/family
source
evidence
authority
freshness
validity
lifecycle
relations
supersession history
why it was retained
```

Support export to Markdown for review/diff.

---

# 41. Improvement 36 — Backup / export / import / restore / wipe

**Priority:** P3

Required operations:

```text
backup
verify backup
restore
export
import
wipe scope
wipe all
rebuild FTS
rebuild vectors
rebuild relation projections
doctor
repair
```

---

# 42. Improvement 37 — Runtime resilience completion

**Priority:** P2/P3

Required behavior:

```text
startup timeout
request timeout
idle timeout
total timeout
cancellation
circuit breaker
provider unavailable
authentication failure
partial completion
fallback
```

Failure must produce typed degradation.

Never silently pretend missing context was complete.

---

# 43. Improvement 38 — Multi-host packaging from one core

**Priority:** P4

Adapters should remain thin.

Do not implement policy separately for hosts.

They should all route to one canonical Membrane implementation.

---

# 44. Improvement 39 — Installed-path qualification

**Priority:** P4

Every major capability must have four proofs:

1. **source proof** — implementation exists and focused tests pass;
2. **integration proof** — actual request path consumes it;
3. **behavior proof** — frozen task demonstrates expected effect;
4. **operational proof** — installed artifact works under realistic resource/failure conditions.

Test native:

```text
macOS
Windows
Linux according to declared support tier
```

Measure:

```text
p50
p95
p99
RSS
CPU
startup
database size
WAL behavior
index size
process count
```

---

# 45. Experimental improvements — only after the core is green

These are not rejected.

They are evidence-gated.

Order:

1. retrieve/no-retrieve classifier;
2. deterministic local query expansion;
3. MMR;
4. bounded graph expansion;
5. local cross-encoder;
6. model-assisted extraction/reflection;
7. multimodal embeddings;
8. community/global graph strategies.

---

# 46. What Membrane should explicitly NOT become

## 46.1 Not a generic vector database platform

Do not add a large backend matrix without a demonstrated product need.

## 46.2 Not a generic graph database platform

Do not require Neo4j/FalkorDB/RDF/SPARQL/ontology engines as architecture.

## 46.3 Not another Blueprint

Do not add a duplicate parser, LSP, SCIP, symbol index, or code graph.

## 46.4 Not an agent framework

No autonomous coding loops, PTY orchestration, generic tool-using agents or multi-agent runtime.

## 46.5 Not an LLM router

That belongs to OmniRouter/harness infrastructure.

## 46.6 Not a network interception proxy

Membrane operates at explicit provider/MCP/context boundaries.

## 46.7 Not LLM-controlled database mutation

Model-assisted reasoning may produce proposals. Deterministic policy validates.

## 46.8 Not prompt optimization as a product

Out of scope.

---

# 47. Recommended implementation sequence

## Release 0 — Authority and measurement

1. establish this document as canonical;
2. supersede stale plans;
3. freeze protocol goldens;
4. build context-quality eval corpus;
5. capture current latency/RSS/token baselines;
6. add write validation;
7. make `no_op` legal.

## Release 1 — Knowledge substrate

8. canonical logical/content/event/artifact identities;
9. evidence references;
10. knowledge family/kind model;
11. lifecycle states;
12. signal sidecar;
13. document-spine integration;
14. source-hash incrementality.

## Release 2 — Truth and memory quality

15. admission policy;
16. deduplication;
17. conflict classification;
18. supersession;
19. expiry;
20. decay;
21. reinforcement;
22. taste/preference semantics;
23. gotcha/procedure semantics;
24. DLP/influence separation.

## Release 3 — Retrieval quality

25. production FTS5/BM25;
26. explicit retrieval channels;
27. retrieval explanation;
28. stable rank fusion;
29. two-phase breadth/depth budget fill;
30. bounded utility adjustments.

## Release 4 — Reversible context compilation

31. canonical `ArtifactRef`;
32. artifact externalization;
33. structure-aware reductions;
34. query-critical verification;
35. resolver restoration;
36. explicit irreversible-transform semantics.

## Release 5 — Source truth

37. Blueprint bridge enrichment;
38. source-anchor resolution;
39. move/rename verification;
40. source drift classification;
41. document claim drift;
42. exact source resolvers.

## Release 6 — Relations

43. narrow relation model;
44. temporal relations;
45. aliases/entities;
46. bounded relation retrieval;
47. relation provenance.

## Release 7 — Session and lifecycle intelligence

48. session packets;
49. controlled promotion;
50. offline session mining;
51. durable job/run records;
52. scheduled maintenance;
53. reversible consolidation.

## Release 8 — Learning and operations

54. feedback binding;
55. lifecycle effectiveness;
56. full retrieval/economics traces;
57. backup/export/import;
58. restore;
59. erasure;
60. index rebuild/doctor.

## Release 9 — Product qualification

61. multi-host installers;
62. Hub human knowledge surface;
63. installed-path tests;
64. Mac/Windows resource qualification;
65. crash/recovery qualification;
66. whole-task benchmarks.

## Release 10 — Experiments

67. retrieve/no-retrieve classifier;
68. deterministic query expansion;
69. MMR;
70. bounded graph expansion;
71. local reranking;
72. model-assisted extraction;
73. multimodal retrieval;
74. global graph techniques.

---

# 48. Migration strategy

## 48.1 Additive first

Introduce new tables/types alongside existing readers.

Do not immediately rewrite `MemoryEntry` and temporal storage.

## 48.2 Shadow new retrieval paths

Example:

```text
cortex_fts_v1:
    off
    shadow
    on

relation_retrieval_v1:
    off
    shadow
    on
```

## 48.3 One cutover authority

Do not maintain permanent dual planners or lifecycle engines.

## 48.4 Rebuildable projections

These must be reconstructable:

```text
FTS
vector index
relation materialization
derived summaries where reproducible
```

## 48.5 Rollback preserves knowledge

Turning a new retrieval feature off must not destroy records written while it was enabled.

---

# 49. Acceptance matrix

## Protocol

- Rust/TypeScript canonical equality;
- golden fixtures;
- backward compatibility;
- explicit protocol version boundaries.

## Scope/security

- traversal;
- symlinks;
- unauthorized repository;
- expired grant;
- cross-scope memory;
- secret-bearing context;
- policy change mid-request.

## Documents

- unchanged document no-op;
- one-section edit only invalidates affected derivatives;
- source anchor survives move where identity allows;
- deleted source invalidates derived evidence;
- superseded decision is not treated as current.

## Memory

- duplicate becomes no-op;
- newer authoritative claim supersedes;
- unresolved contradiction remains represented;
- pinned current knowledge survives decay;
- low-quality transient conversation is rejected.

## Taste

- global vs project preference separated;
- superseded preferences do not leak;
- explicit current user preference outranks weak inferred preference.

## Gotchas

- gotcha surfaces when applicability trigger is present;
- stale source-linked gotcha is demoted/quarantined;
- successful verified reuse reinforces it;
- unrelated historical gotcha does not consume context.

## Retrieval

- exact beats approximate within equivalent authority;
- FTS works without embeddings;
- semantic works without lexical overlap;
- temporal retrieval supports as-of;
- relation expansion is bounded;
- deterministic results independent of provider completion order.

## Compression

- protected spans survive;
- transformations declare lossiness;
- resolver recovers exact source;
- failed recovery is explicit;
- delivered token count reconciles.

## Background jobs

- crash recovery;
- cancellation;
- checkpoint/resume;
- idempotence;
- no prompt-path blocking;
- no duplicate apply.

## Resilience

- provider crash;
- timeout;
- cancellation;
- circuit breaker;
- partial results;
- degraded Blueprint;
- unavailable artifact.

## Recovery

- backup live database;
- restore cleanly;
- rebuild projections;
- erasure remains erased;
- schema rollback boundaries.

## Product

- real installed clients;
- macOS;
- Windows;
- target Linux tier;
- native latency/RSS/process counts.

---

# 50. Success criteria

## Knowledge

Every durable item can answer:

```text
What am I?
Where did I come from?
What supports me?
Whose scope am I in?
How authoritative am I?
When was I observed?
When am I valid?
What replaced me?
What did I derive from?
What lifecycle state am I in?
```

## Documents

The document spine can provide:

```text
exact source
hierarchy
references
semantic retrieval
current hash
authority
derived decisions
derived gotchas
derived constraints
derived taste/preferences where appropriate
```

without creating a second canonical store.

## Retrieval

Membrane can retrieve across:

```text
exact
lexical
semantic
temporal
relations
sessions
documents
memories
preferences/taste
gotchas
live/Git
Blueprint
rules
audit
```

without violating scope, authority or freshness.

## Context

Membrane returns the smallest task-sufficient representation of that evidence.

Large content remains recoverable.

Important content cannot silently disappear.

## Memory

Membrane does not remember everything.

It selectively:

```text
rejects
retains
reinforces
supersedes
expires
decays
archives
quarantines
consolidates
```

and preserves history.

## Evidence

Every consequential operation can explain itself.

Not only:

> "What did we return?"

but:

> "What did we not return, why, what transformation happened, and can we recover the source?"

---

# 51. Final prioritized improvement list

1. Establish one canonical Membrane implementation authority.
2. Freeze current contracts and behavior.
3. Build the canonical context-quality evaluation spine.
4. Add Persist/write validation.
5. Make `no_op` a valid successful write result.
6. Introduce canonical logical/content/event/artifact/evidence identity.
7. Introduce the unified knowledge envelope.
8. Separate canonical knowledge from mutable ranking/lifecycle signals.
9. Make the Markdown/document spine a first-class evidence source.
10. Add hierarchical document/section/claim indexing with exact source ranges.
11. Preserve content-hash/parser-version incremental document processing.
12. Add explicit knowledge family + product-specific knowledge kind taxonomy.
13. Give taste/preferences first-class scoped semantics.
14. Give gotchas/insights first-class procedural/applicability semantics.
15. Implement deterministic admission policy.
16. Implement duplicate/near-duplicate handling.
17. Implement explicit contradiction/conflict records.
18. Generalize immutable supersession across durable knowledge.
19. Add pre-ranking expiry semantics.
20. Add family-specific deterministic decay.
21. Add reinforcement and lifecycle hysteresis.
22. Preserve archive/history rather than hard-deleting routine stale knowledge.
23. Strengthen DLP/redaction before persistence.
24. Separate evidence authority from instructional influence.
25. Add production FTS5/BM25.
26. Turn Cortex retrieval into explicit bounded channels.
27. Add candidate/retrieval explanation traces.
28. Preserve authority/freshness before relevance.
29. Use rank-level fusion rather than arbitrary global raw-score arithmetic.
30. Implement breadth-first-floor → depth-upgrade two-phase budget fill.
31. Generalize resolver-backed delivery into canonical `ArtifactRef`.
32. Externalize large raw payloads before destructive reduction.
33. Add content-type-specific structural reduction.
34. Add query-critical/protected-evidence verification.
35. Restore exact spans automatically when compression loses required evidence.
36. Add source-anchor and source-drift verification.
37. Enrich the Blueprint provider contract rather than duplicating code intelligence.
38. Add a narrow provenance-bearing temporal relation model.
39. Add conservative entity/alias resolution.
40. Add bounded relation retrieval.
41. Add episodic session packets.
42. Add strict session→semantic promotion rules.
43. Add offline retroactive session mining as proposal-only.
44. Add durable Job/Run lifecycle records.
45. Add bounded scheduled background maintenance.
46. Make curation/consolidation reversible.
47. Bind downstream feedback to exact delivered candidate IDs.
48. Track effectiveness without altering authority.
49. Add detailed local retrieval/economics traces plus content-free telemetry.
50. Add read-only human knowledge inspection/export through Hub.
51. Add backup/export/import/restore/wipe as tested product operations.
52. Ensure erasure propagates to every projection and backup lifecycle.
53. Complete provider failure/degradation semantics.
54. Keep adapters thin and ship multi-host integration from one core.
55. Qualify the actual installed path, not only source modules.
56. Measure p50/p95/p99, CPU, RSS, index/storage and token economics.
57. Run memory, retrieval, compression, poisoning, session and whole-task evaluations.
58. Keep advanced adaptive retrieval disabled until it beats deterministic controls.
59. Do not build another Blueprint, graph platform, vector platform, agent framework or LLM router inside Membrane.
60. Preserve Membrane's receipt/evidence architecture as the central product differentiator.

---

# 52. Final architectural principle

The combined competitor research does **not** point toward the system with the most memory types, the largest graph, the most embedding models or the greatest number of retrieval stages.

It points toward something more disciplined:

> **Membrane should know what information exists, where it came from, whether it is current, whether it is trusted, whether it deserves to persist, how it relates to other knowledge, whether the current task needs it, what representation the task can afford, what was removed to make it fit, and how to recover the exact evidence when necessary.**

The document spine, memories, taste, gotchas, sessions and external evidence are different semantic classes, but they belong to the same governed context economy.

Blueprint supplies repository semantics.

Cortex supplies durable knowledge.

Membrane controls the boundary between all available information and the model's finite attention.

Its distinguishing property should therefore remain:

> **Membrane is an auditable context compiler: a system that can prove not only what context it delivered, but why that context was eligible, why it outranked alternatives, what was omitted or transformed, what was remembered, what was forgotten, and which exact evidence supports every durable claim.**
