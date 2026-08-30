#!/usr/bin/env python3
"""
Builds the Ledger held-out retrieval evaluation corpus (ledger-eval-v1).

Reads REPO_ROOT-relative case definitions, validates every quote/heading
against the actual repository files, computes document hashes for the
manifest, and emits train/dev/heldout JSONL files plus manifest.json.

Run from the worktree root:
    python3 build_corpus.py --repo-root /path/to/worktree --out-dir /path/to/out
"""
import argparse
import hashlib
import json
import os
import re
import sys

def slugify(heading: str) -> str:
    # GitHub-style heading slug (approximation; used only as a human alias,
    # never as canonical identity -- consistent with canon S7.3).
    h = heading.strip()
    h = re.sub(r"`", "", h)
    h = h.lower()
    h = re.sub(r"[^\w\s\-]", "", h)
    h = re.sub(r"\s+", "-", h)
    return h

CASES = []
_counter = [0]

def add(split, case_type, query, targets, tags=None, source_corpus="membrane-docs",
        match_mode="single", status="match", notes="", query_script=None,
        plausible_distractors=None):
    _counter[0] += 1
    cid = f"LEDG-EVAL-{_counter[0]:04d}"
    CASES.append({
        "id": cid,
        "split": split,
        "case_type": case_type,
        "tags": tags or [],
        "source_corpus": source_corpus,
        "query": query,
        "query_script": query_script,
        "expected": {
            "status": status,
            "match_mode": match_mode,
            "targets": targets,
            "plausible_distractors": plausible_distractors or [],
        },
        "notes": notes,
    })

def T(document, heading=None, quote=None):
    return {"document": document, "heading": heading, "quote": quote}

# ---------------------------------------------------------------------------
# Frequently referenced documents (paths are repo-root relative)
# ---------------------------------------------------------------------------
LEDGER_CANON = "docs/architecture/subsystems/ledger.md"
LEDGER_REF = "docs/research/legacy-source-corpus/derived-architecture/subsystems/ledger.md"
GUIDE_EVAL = "docs/reference/evaluation/ledger.md"
SYSTEM_MAP = "docs/research/legacy-source-corpus/derived-architecture/system.md"
PULL_REF = "docs/research/legacy-source-corpus/derived-architecture/subsystems/pull.md"
PUSH_REF = "docs/research/legacy-source-corpus/derived-architecture/subsystems/push.md"
CORTEX_REF = "docs/research/legacy-source-corpus/derived-architecture/subsystems/cortex.md"
ADAPT_REF = "docs/architecture/subsystems/adapt.md"
BLUEPRINT_REF = "docs/architecture/subsystems/blueprint.md"
CODERIGHT_REF = "docs/architecture/integrations/coderight.md"
CROSS_SUBSYS = "docs/architecture/cross-subsystem-evidence.md"
DOCTRINE = "docs/architecture/membrane.md"
ARCHITECTURE = "docs/architecture/runtime-truth.md"
PROTOCOL = "docs/reference/protocol/README.md"
PRODUCT = "docs/product/README.md"
DOCS_README = "docs/README.md"
GETTING_STARTED = "docs/product/getting-started.md"
CHANGELOG = "docs/reference/release/CHANGELOG.md"
ROOT_README = "README.md"
SUPPORT_MATRIX = "docs/product/support/matrix.md"
PRICING = "docs/product/support/pricing.md"
HUB_OVERVIEW = "docs/product/hub/overview.md"
HUB_README = "docs/product/hub/README.md"
HUB_NOTIFICATIONS = "docs/product/hub/notifications.md"
BACKUPS = "docs/product/troubleshooting/backups.md"
MEMORY_LIFECYCLE = "docs/product/memory/lifecycle.md"
RESOURCES = "docs/reference/protocol/resources.md"
INSTALL_REGISTRY = "docs/product/installation/registry.md"
ROOTS = "docs/product/installation/roots.md"
CHECKPOINT = "docs/product/workflows/checkpoint.md"
SUPPORT_BOUNDARIES = "docs/product/support/boundaries.md"
PAPER_ADAS = "docs/research/legacy-source-corpus/papers/automated-design-agentic-systems_2408.08435.md"
PAPER_MANUS = "docs/research/legacy-source-corpus/papers/articles/manus-context-engineering.md"
PAPER_DGM = "docs/research/legacy-source-corpus/papers/darwin-godel-machine_2505.22954.md"

# ===========================================================================
# 1. exact_document  (14: 5 train / 5 dev / 4 heldout)
# ===========================================================================
add("train", "exact_document",
    "Ledger Markdown Indexing and Document Navigation Canon",
    [T(LEDGER_CANON, "Ledger Markdown Indexing and Document Navigation Canon")],
    tags=["exact_document"],
    notes="Direct title match against the canonical Ledger architecture doctrine.")

add("train", "exact_document",
    "Membrane Stable Roots documentation",
    [T(ROOTS, "Membrane Stable Roots")],
    tags=["exact_document"],
    notes="Title-level match; MBR-106 stable-roots contract doc.")

add("train", "exact_document",
    "Membrane changelog",
    [T(CHANGELOG, "Changelog")],
    tags=["exact_document"])

add("train", "exact_document",
    "Push reversible reduction and recovery subsystem reference",
    [T(PUSH_REF, "Push — Reversible Reduction & Recovery")],
    tags=["exact_document"])

add("train", "exact_document",
    "Pricing and service boundaries document",
    [T(PRICING, "Pricing & service boundaries")],
    tags=["exact_document"])

add("dev", "exact_document",
    "Cortex durable knowledge subsystem reference",
    [T(CORTEX_REF, "Cortex — Durable Knowledge")],
    tags=["exact_document"])

add("dev", "exact_document",
    "Membrane system map document",
    [T(SYSTEM_MAP, "Membrane — System Map")],
    tags=["exact_document"])

add("dev", "exact_document",
    "Ledger qualification entrypoint document",
    [T(GUIDE_EVAL, "Ledger qualification entrypoint")],
    tags=["exact_document"],
    notes="Current qualification entrypoint uses canonical Ledger naming.")

add("dev", "exact_document",
    "checkpoint MCP workflow prompt documentation",
    [T(CHECKPOINT, "`checkpoint` — A0 session checkpoint save prompt")],
    tags=["exact_document"])

add("dev", "exact_document",
    "Membrane MCP resources and templates document",
    [T(RESOURCES, "Membrane MCP resources and templates (MBR-304)")],
    tags=["exact_document"])

add("heldout", "exact_document",
    "Membrane in five minutes getting-started guide",
    [T(GETTING_STARTED, "Membrane in five minutes (MBR-1002)")],
    tags=["exact_document"])

add("heldout", "exact_document",
    "Hub overview document",
    [T(HUB_OVERVIEW, "Hub overview")],
    tags=["exact_document"])

add("heldout", "exact_document",
    "Backups troubleshooting document",
    [T(BACKUPS, "Backups")],
    tags=["exact_document"])

add("heldout", "exact_document",
    "MCP Registry metadata document",
    [T(INSTALL_REGISTRY, "MCP Registry metadata (MBR-907)")],
    tags=["exact_document"])

# ===========================================================================
# 2. exact_section  (12: 4 / 4 / 4)
# ===========================================================================
add("train", "exact_section",
    "Ledger canon section 8.2 developer-identifier expansion",
    [T(LEDGER_CANON, "8.2 Developer-identifier expansion")],
    tags=["exact_section"])

add("train", "exact_section",
    "Locked invariants section of the Ledger canon",
    [T(LEDGER_CANON, "4. Locked invariants")],
    tags=["exact_section"])

add("train", "exact_section",
    "Corpus size and statistical gate section of the Ledger canon",
    [T(LEDGER_CANON, "12.2 Corpus size and statistical gate")],
    tags=["exact_section"])

add("train", "exact_section",
    "Protected material section of the Push subsystem reference",
    [T(PUSH_REF, "Protected material")],
    tags=["exact_section"])

add("dev", "exact_section",
    "Ledger canon section 2.1 canonical name",
    [T(LEDGER_CANON, "2.1 Canonical name")],
    tags=["exact_section"])

add("dev", "exact_section",
    "Session document projection recallability section of the Ledger canon",
    [T(LEDGER_CANON, "14.3 Recallability")],
    tags=["exact_section"])

add("dev", "exact_section",
    "Ledger canon BM25 weights section",
    [T(LEDGER_CANON, "9.3 BM25 weights")],
    tags=["exact_section"])

add("dev", "exact_section",
    "Canonical runtime namespace section of the Cortex reference",
    [T(CORTEX_REF, "Canonical runtime namespace")],
    tags=["exact_section"])

add("heldout", "exact_section",
    "Ledger canon title-chain contextualization experiment section",
    [T(LEDGER_CANON, "11. Title-chain contextualization is an experiment")],
    tags=["exact_section"])

add("heldout", "exact_section",
    "Ledger canon rejected-designs section",
    [T(LEDGER_CANON, "20. Rejected designs")],
    tags=["exact_section"])

add("heldout", "exact_section",
    "Query operator safety section of the Ledger canon",
    [T(LEDGER_CANON, "8.4 Query operator safety")],
    tags=["exact_section"])

add("heldout", "exact_section",
    "Environment overrides section of the stable roots document",
    [T(ROOTS, "Environment overrides")],
    tags=["exact_section"])

# ===========================================================================
# 3. table_content  (9: 3 / 3 / 3)
# ===========================================================================
add("train", "table_content",
    "What support tier does the windows/cursor installed-path pair have in the support matrix?",
    [T(SUPPORT_MATRIX, "Support-tier matrix",
       "| windows | cursor | installed-path | unavailable | windows receipt unavailable |")],
    tags=["table_content"])

add("train", "table_content",
    "Does the cache root survive an update, and does it survive an uninstall?",
    [T(ROOTS, "The four roots",
       "| `cache`  | regenerable caches (fastembed, downloaded models) | yes            | no                 |")],
    tags=["table_content"])

add("train", "table_content",
    "Which budget lane consumes zero tokens but reports deliveredChars only?",
    [T(ARCHITECTURE, "Cross-provider budget model",
       "| `resolver_backed` | Resolver reference only; the agent retrieves on demand | zero tokens; `deliveredChars` only |")],
    tags=["table_content"])

add("dev", "table_content",
    "In the pricing capability table, what is the boundary for Team sync?",
    [T(PRICING, "Optional paid capabilities",
       "| Team sync | **undecided** | Could coordinate opted-in repositories; must not replace local authority or receipts. |")],
    tags=["table_content"])

add("dev", "table_content",
    "Which issuer grants the lease.read access needed for the lease-status resource?",
    [T(RESOURCES, "Supported grant types",
       "| `lease.read`       | Supervisor-resident admission gate       | `lease-status`                                       |")],
    tags=["table_content"])

add("dev", "table_content",
    "In the README's six-axes table, what does the Cortex / Persist row say it keeps useful across sessions?",
    [T(ROOT_README, "Six axes",
       "| **Cortex / Persist** | Keeps governed durable decisions, preferences, and lessons useful across sessions and machines |")],
    tags=["table_content"])

add("heldout", "table_content",
    "Per the process plane separation table, what does the Control plane write to?",
    [T(ARCHITECTURE, "Process plane separation (MBR-108)",
       "| Control | daemon admission, worker supervision, & health; tray owns process lifecycle externally | Data | Data |")],
    tags=["table_content"])

add("heldout", "table_content",
    "Per the system map's store-ownership table, who owns cortex-engine.db and is it rebuildable?",
    [T(SYSTEM_MAP, "4. Store ownership",
       "| `cortex-engine.db` | Cortex | authored durable knowledge; irreplaceable |")],
    tags=["table_content"])

add("heldout", "table_content",
    "Per the system map's one-question-per-subsystem table, what does Ledger not own?",
    [T(SYSTEM_MAP, "2. One question per subsystem",
       "| **Ledger** | Where in the documents is the relevant material? | document/section index, stable anchors, hash-bound references, document navigation, rebuildable index store | source-document authority, document truth, durable knowledge, final admission |")],
    tags=["table_content"])

# ===========================================================================
# 4. fenced_code  (9: 3 / 3 / 3)
# ===========================================================================
add("train", "fenced_code",
    "What native command & arguments configure MCP in the getting-started guide?",
    [T(GETTING_STARTED, "2. Configure MCP (0:45)", '"command": "membrane",\n      "args": ["stdio-mcp"]')],
    tags=["fenced_code"])

add("train", "fenced_code",
    "What does the JSON output of `membrane cli doctor paths` look like?",
    [T(ROOTS, "Inspecting a live install", "\"schemaVersion\": 1,\n  \"product\": \"Membrane\",")],
    tags=["fenced_code"])

add("train", "fenced_code",
    "Show the code block listing Cortex's canonical runtime namespace (crates, binaries, environment, store).",
    [T(CORTEX_REF, "Canonical runtime namespace", "cortex-engine.db")],
    tags=["fenced_code"])

add("dev", "fenced_code",
    "What are the two deterministic offline fixture commands at the end of the getting-started guide?",
    [T(GETTING_STARTED, "Deterministic offline fixture",
       "node docs/reference/examples/quickstart/run.mjs\nnode docs/reference/examples/quickstart/run.mjs --degraded")],
    tags=["fenced_code"])

add("dev", "fenced_code",
    "What does the example prompts/get JSON-RPC request for the checkpoint prompt look like?",
    [T(CHECKPOINT, "Example invocation", "\"name\": \"checkpoint\",")],
    tags=["fenced_code"])

add("dev", "fenced_code",
    "What shell commands does the README's 'Running it' section give for building and testing?",
    [T(ROOT_README, "Running it", "rightkit cargo build --workspace                 # Membrane engine")],
    tags=["fenced_code"])

add("heldout", "fenced_code",
    "What does the resource wire-shape JSON template in the MCP resources doc declare for authorityEscalation?",
    [T(RESOURCES, "Resource wire shape", "\"authorityEscalation\": false,        // resources never escalate")],
    tags=["fenced_code"])

add("heldout", "fenced_code",
    "What are the first two nodes of the mermaid flowchart in the README's packet-assembly section?",
    [T(ROOT_README, "How a packet is assembled", "T[task + repository] --> SG[ScopeGrant]")],
    tags=["fenced_code"])

add("heldout", "fenced_code",
    "What does the boundary-direction ASCII diagram in the system map show flowing into Push?",
    [T(SYSTEM_MAP, "5. Boundary direction", "Push executes selected representation")],
    tags=["fenced_code"])

# ===========================================================================
# 5. list_item  (9: 3 / 3 / 3)
# ===========================================================================
add("train", "list_item",
    "What four environment variables override Membrane's stable roots for tests?",
    [T(ROOTS, "Environment overrides", "- `MEMBRANE_CONFIG_ROOT`")],
    tags=["list_item", "identifier"])

add("train", "list_item",
    "What write-proposed operations does the checkpoint prompt explicitly forbid calling?",
    [T(CHECKPOINT, "Authority scope", "- `membrane_knowledge_propose` (durable knowledge proposal)")],
    tags=["list_item"])

add("train", "list_item",
    "What is the third step in Push's ordered reversible reduction ladder?",
    [T(PUSH_REF, "Owns", "3. deterministic noise removal;")],
    tags=["list_item"])

add("dev", "list_item",
    "What kinds of material does Push protect at minimum?",
    [T(PUSH_REF, "Protected material", "- identifiers;")],
    tags=["list_item"])

add("dev", "list_item",
    "What optional fields can Ledger's persisted node contract carry?",
    [T(LEDGER_CANON, "7.2 Persisted node contract", "human_anchor_aliases[]")],
    tags=["list_item"])

add("dev", "list_item",
    "What block types must the Ledger's source-positioned AST projection cover?",
    [T(LEDGER_CANON, "7.1 Parse once, preserve source positions", "- fenced code;")],
    tags=["list_item"])

add("heldout", "list_item",
    "What four resources does the Membrane MCP `resources/list` bullet list name?",
    [T(RESOURCES, "Why resources, not tools", "- `resources-index` — what resources exist, what grants does each require,")],
    tags=["list_item"])

add("heldout", "list_item",
    "Where does docs/README.md place landed memory documentation?",
    [T(DOCS_README, "Layout", "[memory](product/memory/)")],
    tags=["list_item"])

add("heldout", "list_item",
    "What is Cortex's Definition-of-Done checklist item about FTS5/BM25?",
    [T(CORTEX_REF, "Definition of Done", "- [ ] FTS5/BM25 production projection exists and retrieval works with embeddings disabled.")],
    tags=["list_item"])

# ===========================================================================
# 6. blockquote  (9: 3 / 3 / 3)
# ===========================================================================
add("train", "blockquote",
    "What one-sentence question does Ledger canonically answer?",
    [T(LEDGER_CANON, "Executive decision",
       "Where in the registered document corpus is the relevant material, and can the exact current source bytes be resolved safely?")],
    tags=["blockquote"])

add("train", "blockquote",
    "What is the Ledger canon's final canonical statement?",
    [T(LEDGER_CANON, "22. Final canonical statement",
       "Ledger is Membrane's document registry, Markdown structural indexing, navigation, retrieval, and exact source-resolution subsystem.")],
    tags=["blockquote"])

add("train", "blockquote",
    "What one-line question does the Cortex subsystem reference say it answers?",
    [T(CORTEX_REF, "Purpose", "What do we durably know?")],
    tags=["blockquote"])

add("dev", "blockquote",
    "What one-line question does the Push subsystem reference say it answers?",
    [T(PUSH_REF, "Purpose",
       "How can the Pull-selected context be made smaller without destroying anything the task may need back?")],
    tags=["blockquote"])

add("dev", "blockquote",
    "What does the CodeRight integration doc's final canonical statement say Ledger does?",
    [T(CODERIGHT_REF, "20. Final canonical integration statement", "Ledger indexes and resolves registered documents")],
    tags=["blockquote"])

add("dev", "blockquote",
    "What does the Membrane doctrine's executive-decision blockquote say Membrane decides?",
    [T(DOCTRINE, "0. Executive decision",
       "Membrane decides what deserves the agent's limited attention now, in what form, under whose authority, and records exactly why.")],
    tags=["blockquote"])

add("heldout", "blockquote",
    "What does the stable-roots document's acceptance blockquote require of install/update/rollback/uninstall?",
    [T(ROOTS, "Acceptance", "Install, update, rollback, and uninstall preserve user data and")],
    tags=["blockquote"])

add("heldout", "blockquote",
    "What does the Blueprint canon say physical co-location does not imply?",
    [T(BLUEPRINT_REF, "0.2 Physical co-location does not change ownership", "Physical co-location does not imply semantic ownership.")],
    tags=["blockquote"])

add("heldout", "blockquote",
    "What single-sentence system rule opens the Membrane system map?",
    [T(SYSTEM_MAP, "1. System rule",
       "Membrane is the parent context system. Pull, Push, Cortex, Blueprint, Ledger, and Adapt are its six named subsystems.")],
    tags=["blockquote"])

# ===========================================================================
# 7. link_reference  (9: 3 / 3 / 3)
# ===========================================================================
add("train", "link_reference",
    "Which current contract does Hub documentation link for lifecycle conformance?",
    [T(HUB_README, None, "[tray–daemon runtime contract](../../architecture/runtime/tray-daemon-contract.md)")],
    tags=["link_reference"],
    notes="The link target resolves to sole current tray–daemon runtime contract.")

add("train", "link_reference",
    "Which file does docs/README.md link to for the document registry/navigation/index reference?",
    [T(DOCS_README, "Architecture authorities", "[Ledger architecture](architecture/subsystems/ledger.md)")],
    tags=["link_reference"])

add("train", "link_reference",
    "Which schema file does the checkpoint prompt document cite as its source of truth?",
    [T(CHECKPOINT, "Source of truth", "[`schemas/registry/prompts/checkpoint.v1.json`](../../../schemas/registry/prompts/checkpoint.v1.json)")],
    tags=["link_reference"])

add("dev", "link_reference",
    "Which document does the README link to for the full support-tier matrix table?",
    [T(ROOT_README, "Support tier matrix", "[docs/product/support/matrix.md](docs/product/support/matrix.md)")],
    tags=["link_reference"])

add("dev", "link_reference",
    "Which directory does docs/README.md link for atomic capability state?",
    [T(DOCS_README, "Architecture authorities", "[Atomic capability canons](canon/)")],
    tags=["link_reference"])

add("dev", "link_reference",
    "Which generated doc does the root README cite as the source of truth for the 17 MCP tools?",
    [T(ROOT_README, "Inside", "[docs/reference/product-truth.md](docs/reference/product-truth.md)")],
    tags=["link_reference"])

add("heldout", "link_reference",
    "Which test file does docs/reference/protocol/resources.md cite as the MBR-303 prompt-parity precedent?",
    [T(RESOURCES, "Cross-references", "`tests/mcp-prompts/prompts.parity.test.mjs`")],
    tags=["link_reference"])

add("heldout", "link_reference",
    "Which document does pricing documentation link to for the public-facing support-boundary summary?",
    [T(PRICING, "No invented commercial claims", "[`boundaries.md`](boundaries.md)")],
    tags=["link_reference"])

add("heldout", "link_reference",
    "Which document does docs/README.md link as current Adapt architecture?",
    [T(DOCS_README, "Architecture authorities", "[Adapt architecture](architecture/subsystems/adapt.md)")],
    tags=["link_reference"])

# ===========================================================================
# 8. negative_no_answer  (9: 3 / 3 / 3)
# ===========================================================================
add("train", "negative_no_answer",
    "What Kubernetes namespace does Membrane Hub deploy into?",
    [], status="no_match", tags=["negative"],
    notes="Membrane is explicitly local-first with a resident macOS service; no Kubernetes/orchestration content exists anywhere in the corpus.")

add("train", "negative_no_answer",
    "What cloud region does Membrane's hosted context service run in?",
    [], status="no_match", tags=["negative"],
    plausible_distractors=[PRICING],
    notes="pricing.md states hosted/paid capability is undecided/unavailable, but names no region and no hosted service exists; a lexical matcher may over-rank pricing.md.")

add("train", "negative_no_answer",
    "What are Ledger's concrete default BM25 k1 and b parameter values?",
    [], status="no_match", tags=["negative"],
    plausible_distractors=[LEDGER_CANON],
    notes="Canon S9.3 says BM25 weights are tunable, to be frozen after dev-split tuning; no numeric defaults are given anywhere in the corpus.")

add("dev", "negative_no_answer",
    "What is the maximum number of documents Ledger can index?",
    [], status="no_match", tags=["negative"],
    notes="No numeric corpus-size ceiling is stated anywhere in the Ledger canon or reference doc.")

add("dev", "negative_no_answer",
    "What is the GraphQL schema for Membrane's context API?",
    [], status="no_match", tags=["negative"],
    plausible_distractors=[PROTOCOL, RESOURCES],
    notes="Membrane's protocol surface is MCP/JSON-RPC only; no GraphQL schema exists in the corpus.")

add("dev", "negative_no_answer",
    "What is the fixed minimum number of held-out queries the Ledger evaluation canon requires?",
    [], status="no_match", tags=["negative"],
    plausible_distractors=[LEDGER_CANON],
    notes="Canon S12.2 explicitly rejects a fixed threshold ('Do not assume 50-100 questions can support arbitrary small quality thresholds') in favor of an MDE or paired-bootstrap gate; it states no fixed number. A naive matcher will likely surface S12.2 anyway; correct behavior distinguishes 'topically relevant' from 'answers the question'.")

add("heldout", "negative_no_answer",
    "What is Membrane Hub's OAuth client-secret rotation procedure?",
    [], status="no_match", tags=["negative"],
    notes="No OAuth flow of any kind is documented anywhere in the corpus.")

add("heldout", "negative_no_answer",
    "What is the exact CREATE TABLE SQL DDL for Ledger's FTS5 table?",
    [], status="no_match", tags=["negative"],
    plausible_distractors=[LEDGER_CANON],
    notes="Canon S9.1 lists weighted field names (path/title/heading/body/identifier_aliases) but never gives literal SQL DDL.")

add("heldout", "negative_no_answer",
    "What is the refund policy for a paid Membrane Pro subscription?",
    [], status="no_match", tags=["negative"],
    plausible_distractors=[PRICING],
    notes="pricing.md states price/plan names are unavailable/undecided; no subscription product or refund policy exists to describe.")

# ===========================================================================
# 9. paraphrase  (9: 3 / 3 / 3) -- low lexical overlap with the target text
# ===========================================================================
add("train", "paraphrase",
    "How do I know Membrane didn't just make something up when it couldn't find real evidence?",
    [T(ROOT_README, "What makes it different",
       "The receipt records what was skipped, timed out, inaccessible, or dropped for budget")],
    tags=["paraphrase"])

add("train", "paraphrase",
    "If two pieces of evidence disagree, does the newer-looking one automatically win just because it seems similar to the task?",
    [T(ROOT_README, "What makes it different", "A stale but semantically-similar candidate cannot silently outrank current code.")],
    tags=["paraphrase"])

add("train", "paraphrase",
    "Is Ledger allowed to decide whether a document's claims are actually true about the code?",
    [T(LEDGER_REF, "Invariants", "Ledger does not decide whether a document claim is true against code.")],
    tags=["paraphrase"])

add("dev", "paraphrase",
    "Can one subsystem just peek directly into another subsystem's private database as a shortcut?",
    [T(SYSTEM_MAP, "5. Boundary direction", "No subsystem opens another subsystem's store as an implementation shortcut.")],
    tags=["paraphrase"])

add("dev", "paraphrase",
    "Is having the code merged and present on disk enough on its own to say a feature is actually finished and working?",
    [T(LEDGER_CANON, "4. Locked invariants", "An index existing on disk is not evidence it is shipped.")],
    tags=["paraphrase"])

add("dev", "paraphrase",
    "When the config and data folders end up at the exact same path on a Mac, does the runtime stop caring about the difference between them internally?",
    [T(ROOTS, "Platform mapping", "as separate logical roots so cache eviction never touches data.")],
    tags=["paraphrase"])

add("heldout", "paraphrase",
    "If I only type one or two words, should the system go crawl the whole document graph hunting for an answer?",
    [T(LEDGER_CANON, "8.3 Short queries", "Do not treat short queries as an excuse for broad full-corpus traversal.")],
    tags=["paraphrase"])

add("heldout", "paraphrase",
    "Does the free local correctness guarantee ever depend on whether someone actually paid for anything?",
    [T(PRICING, "Free local baseline", "is not conditioned on payment in this checkout")],
    tags=["paraphrase"])

add("heldout", "paraphrase",
    "When a change is rolled back, do the four stable directories get wiped and rebuilt, or left exactly as they are?",
    [T(ROOTS, "How install / update / rollback / uninstall use the roots",
       "the previous binary is\n  restored, the four roots are left alone")],
    tags=["paraphrase"])

# ===========================================================================
# 10. non_ascii_cjk  (6: 2 / 2 / 2)  -- scarce real content, noted in README
# ===========================================================================
add("train", "non_ascii_cjk",
    "この数学の問題を解いてください",
    [T(PAPER_ADAS, "An example question from MGSM:", "この数学の問題を解いてください。")],
    tags=["cjk", "japanese"], source_corpus="research-papers", query_script="ja",
    notes="Exact Japanese MGSM benchmark question embedded in an ICLR paper appendix; tests that a CJK-only query does not silently tokenize to zero terms (canon S8.1/S1).")

add("train", "non_ascii_cjk",
    "简体中文",
    [T(PAPER_MANUS, None, "简体中文")],
    tags=["cjk", "chinese"], source_corpus="research-papers", query_script="zh",
    notes="Simplified-Chinese language-name token from the scraped Manus blog's language-switcher boilerplate; no clean GFM heading precedes it (page chrome before the first '##').")

add("dev", "non_ascii_cjk",
    "日本語",
    [T(PAPER_MANUS, None, "日本語")],
    tags=["cjk", "japanese"], source_corpus="research-papers", query_script="ja",
    notes="Japanese language-name token, same language-switcher line as the Chinese-name case; distinct query string, same source document (CJK source material is scarce in this repo).")

add("dev", "non_ascii_cjk",
    "ペットのウサギとペットの犬と猫",
    [T(PAPER_ADAS, "An example question from MGSM:", "ペットのウサギの数がペットの犬と猫を合わせた数")],
    tags=["cjk", "japanese"], source_corpus="research-papers", query_script="ja",
    notes="Paraphrase-style Japanese query over the same MGSM passage as the train-split case, worded differently to avoid split leakage.")

add("heldout", "non_ascii_cjk",
    "한국어",
    [T(PAPER_MANUS, None, "한국어")],
    tags=["cjk", "korean"], source_corpus="research-papers", query_script="ko",
    notes="Korean language-name token from the same language-switcher boilerplate line.")

add("heldout", "non_ascii_cjk",
    "近所には何匹のペットがいますか",
    [T(PAPER_ADAS, "An example question from MGSM:", "全部で近所には何匹のペットがいますか")],
    tags=["cjk", "japanese"], source_corpus="research-papers", query_script="ja",
    notes="Tail clause of the same MGSM question, held out untouched for final evaluation.")

# ===========================================================================
# 11. mixed_script  (6: 2 / 2 / 2)
# ===========================================================================
add("train", "mixed_script",
    "MGSM benchmark question この数学の問題を解いてください example",
    [T(PAPER_ADAS, "An example question from MGSM:", "この数学の問題を解いてください")],
    tags=["mixed_script"], source_corpus="research-papers", query_script="mixed-en-ja")

add("train", "mixed_script",
    "Gödel machine self-referential rewriting Schmidhuber",
    [T(PAPER_DGM, None, "The Gödel machine (Schmidhuber, 2007) proposed a theoretical alternative")],
    tags=["mixed_script"], source_corpus="research-papers", query_script="mixed-en-accented",
    notes="No GFM headings exist in this PDF-extracted paper body; only YAML frontmatter title 'darwin-godel-machine'.")

add("dev", "mixed_script",
    "简体中文 language switcher Manus blog article",
    [T(PAPER_MANUS, None, "简体中文")],
    tags=["mixed_script"], source_corpus="research-papers", query_script="mixed-en-zh")

add("dev", "mixed_script",
    "Jürgen Schmidhuber Darwin Gödel Machine paper",
    [T(PAPER_DGM, None, "DARWIN GÖDEL MACHINE: OPEN-ENDED EVOLUTION")],
    tags=["mixed_script"], source_corpus="research-papers", query_script="mixed-en-accented")

add("heldout", "mixed_script",
    "日本語 한국어 language list Context Engineering article",
    [T(PAPER_MANUS, None, "日本語한국어")],
    tags=["mixed_script"], source_corpus="research-papers", query_script="mixed-en-ja-ko")

add("heldout", "mixed_script",
    "MGSM 犬と猫 dogs and cats pets benchmark",
    [T(PAPER_ADAS, "An example question from MGSM:", "犬と猫を合わせた数")],
    tags=["mixed_script"], source_corpus="research-papers", query_script="mixed-en-ja")

# ===========================================================================
# 12. identifier_snake_case  (9: 3 / 3 / 3)
# ===========================================================================
add("train", "identifier_snake_case",
    "doc_candidate_provider",
    [T(MEMORY_LIFECYCLE, None, "doc_candidate_provider.rs")],
    tags=["identifier", "snake_case"])

add("train", "identifier_snake_case",
    "select_shadow",
    [T(MEMORY_LIFECYCLE, None, "DocCandidateProvider::select_shadow")],
    tags=["identifier", "snake_case"])

add("train", "identifier_snake_case",
    "plane_for_path",
    [T(ARCHITECTURE, "Process plane separation (MBR-108)", "plane_for_path")],
    tags=["identifier", "snake_case"])

add("dev", "identifier_snake_case",
    "register_receipt_owned",
    [T(ROOTS, "The receipt and uninstall residue", "`membrane_runtime::receipt::register_receipt_owned`")],
    tags=["identifier", "snake_case"])

add("dev", "identifier_snake_case",
    "retrieve_hybrid_indexed",
    [T(CHANGELOG, "2026-08-02 — Vector dispatch v2 default-on", "retrieve_hybrid_indexed")],
    tags=["identifier", "snake_case"])

add("dev", "identifier_snake_case",
    "read_payload",
    [T(RESOURCES, "Source of truth and round-trip", "`list_payload`, `read_payload`, `read_result_payload`")],
    tags=["identifier", "snake_case"])

add("heldout", "identifier_snake_case",
    "doc_spine",
    [T(LEDGER_CANON, "8.2 Developer-identifier expansion", "doc_spine -> doc_spine, doc, spine")],
    tags=["identifier", "snake_case"])

add("heldout", "identifier_snake_case",
    "build_session_projection",
    [T(LEDGER_CANON, "2.2 Existing session-ledger collision", "build_session_projection")],
    tags=["identifier", "snake_case"])

add("heldout", "identifier_snake_case",
    "is_doc_provider_enabled",
    [T(MEMORY_LIFECYCLE, None, "is_doc_provider_enabled()")],
    tags=["identifier", "snake_case"])

# ===========================================================================
# 13. identifier_camel_case  (9: 3 / 3 / 3)
# ===========================================================================
add("train", "identifier_camel_case",
    "GuideDb",
    [T(LEDGER_CANON, "2.3 Rename surface", "`GuideDb` and Guide-owned table/file names;")],
    tags=["identifier", "camel_case"])

add("train", "identifier_camel_case",
    "LedgerDb",
    [T(LEDGER_REF, "Owns", "`LedgerDb` at `cache_root()/ledger-index.sqlite3`")],
    tags=["identifier", "camel_case"])

add("train", "identifier_camel_case",
    "authorityEscalation",
    [T(RESOURCES, "Resource wire shape", "\"authorityEscalation\": false,        // resources never escalate")],
    tags=["identifier", "camel_case"])

add("dev", "identifier_camel_case",
    "schemaVersion",
    [T(ROOTS, "Inspecting a live install", "\"schemaVersion\": 1,")],
    tags=["identifier", "camel_case"])

add("dev", "identifier_camel_case",
    "checkpointLabel",
    [T(CHECKPOINT, "Example invocation", "\"checkpointLabel\": \"after-mbr-303-impl\",")],
    tags=["identifier", "camel_case"])

add("dev", "identifier_camel_case",
    "authorityScope",
    [T(CHECKPOINT, "Authority scope", "`authorityScope`: `write-proposed`")],
    tags=["identifier", "camel_case"])

add("heldout", "identifier_camel_case",
    "namespaceStatus artifactStatus",
    [T(INSTALL_REGISTRY, "What `server.json` asserts, and how each part is checked",
       "`server.publication.namespaceStatus` and\n   `.artifactStatus`")],
    tags=["identifier", "camel_case"])

add("heldout", "identifier_camel_case",
    "platformReceipt",
    [T(INSTALL_REGISTRY, "What `server.json` asserts, and how each part is checked", "`platformReceipt` field is `null` today")],
    tags=["identifier", "camel_case"])

add("heldout", "identifier_camel_case",
    "BudgetLaneKind",
    [T(ARCHITECTURE, "Cross-provider budget model", "`BudgetLaneKind`")],
    tags=["identifier", "camel_case"])

# ===========================================================================
# 14. identifier_path_fragment  (9: 3 / 3 / 3)
# ===========================================================================
add("train", "identifier_path_fragment",
    "src/ledger/doc_spine.rs",
    [T(LEDGER_CANON, "8.2 Developer-identifier expansion", "src/ledger/doc_spine.rs")],
    tags=["identifier", "path_fragment"])

add("train", "identifier_path_fragment",
    "engine/crates/membrane-runtime/src/ledger/doc_spine.rs",
    [T(DOCTRINE, "20. Canonical file-level ownership map",
       "`engine/crates/membrane-runtime/src/ledger/{outline,identifier,doc_spine,doc_projection,doc_shadow,doc_candidate_provider}.rs`")],
    tags=["identifier", "path_fragment"])

add("train", "identifier_path_fragment",
    "engine/crates/membrane-protocol/src/types.rs",
    [T(ARCHITECTURE, "Cross-provider budget model", "`engine/crates/membrane-protocol/src/types.rs`")],
    tags=["identifier", "path_fragment"])

add("dev", "identifier_path_fragment",
    "engine/crates/membrane-core/src/reconcile.rs",
    [T(ARCHITECTURE, "Cross-provider budget model", "`engine/crates/membrane-core/src/reconcile.rs`")],
    tags=["identifier", "path_fragment"])

add("dev", "identifier_path_fragment",
    "schemas/registry/resources/resources-index.v1.json",
    [T(RESOURCES, "Resource wire shape", "`schemas/registry/resources/resources-index.v1.json`")],
    tags=["identifier", "path_fragment"])

add("dev", "identifier_path_fragment",
    "~/Library/Application Support/Membrane",
    [T(ROOTS, "Platform mapping", "~/Library/Application Support/Membrane")],
    tags=["identifier", "path_fragment"])

add("heldout", "identifier_path_fragment",
    "engine/crates/membrane-mcp/src/resources.rs",
    [T(RESOURCES, "Source of truth and round-trip", "`engine/crates/membrane-mcp/src/resources.rs`")],
    tags=["identifier", "path_fragment"])

add("heldout", "identifier_path_fragment",
    "scripts/tools/productization/generate-product-truth.mjs",
    [T(ARCHITECTURE, None, "scripts/tools/productization/generate-product-truth.mjs")],
    tags=["identifier", "path_fragment"])

add("heldout", "identifier_path_fragment",
    "docs/archive/superseded/reference/deferred-surfaces.md",
    [T(MEMORY_LIFECYCLE, None, "docs/archive/superseded/reference/deferred-surfaces.md")],
    tags=["identifier", "path_fragment"])

# ===========================================================================
# 15. short_query  (9: 3 / 3 / 3) -- 1-2 terms
# ===========================================================================
add("train", "short_query",
    "LedgerDb",
    [T(LEDGER_REF, "Owns", "`LedgerDb`")],
    tags=["short_query", "identifier"])

add("train", "short_query",
    "doc_spine",
    [T(LEDGER_CANON, "8.2 Developer-identifier expansion", "doc_spine")],
    tags=["short_query", "identifier"])

add("train", "short_query",
    "BM25 weights",
    [T(LEDGER_CANON, "9.3 BM25 weights", "BM25 field weights are tunable parameters")],
    tags=["short_query"])

add("dev", "short_query",
    "stable roots",
    [T(ROOTS, "Membrane Stable Roots", "Membrane Stable Roots")],
    tags=["short_query"])

add("dev", "short_query",
    "checkpoint prompt",
    [T(CHECKPOINT, "`checkpoint` — A0 session checkpoint save prompt", "checkpoint")],
    tags=["short_query"])

add("dev", "short_query",
    "held-out",
    [T(LEDGER_CANON, "12.1 Do not tune on held-out", "held-out")],
    tags=["short_query"])

add("heldout", "short_query",
    "hub_inactive",
    [T(GETTING_STARTED, "6. Prove fail-closed lifecycle (4:30)", "hub_inactive")],
    tags=["short_query", "identifier"])

add("heldout", "short_query",
    "support matrix",
    [T(SUPPORT_MATRIX, "Support-tier matrix", "Support-tier matrix")],
    tags=["short_query"])

add("heldout", "short_query",
    "session projection",
    [T(LEDGER_CANON, "14. Session document projection", "session document projection")],
    tags=["short_query"])

# ===========================================================================
# 16. multi_section_synthesis  (9: 3 / 3 / 3)
# ===========================================================================
add("train", "multi_section_synthesis",
    "Compare what Ledger owns to what Cortex owns, per their subsystem references.",
    [T(LEDGER_REF, "Owns"), T(CORTEX_REF, "Owns")],
    match_mode="all_of", tags=["multi_section_synthesis"])

add("train", "multi_section_synthesis",
    "Per the Ledger canon and the system map together, which subsystem owns repository truth and which owns document navigation?",
    [T(LEDGER_CANON, "3. Canonical ownership"), T(SYSTEM_MAP, "2. One question per subsystem")],
    match_mode="all_of", tags=["multi_section_synthesis"])

add("train", "multi_section_synthesis",
    "Using getting-started steps 3 and 6 together, what proves delivery & what proves fail-closed lifecycle?",
    [T(GETTING_STARTED, "3. Request first packet (1:30)"), T(GETTING_STARTED, "6. Prove fail-closed lifecycle (4:30)")],
    match_mode="all_of", tags=["multi_section_synthesis"])

add("dev", "multi_section_synthesis",
    "Trace a Ledger candidate from Ledger's local FTS through Pull to Push, using the canon's target architecture plus Pull's and Push's own references.",
    [T(LEDGER_CANON, "6. Selected target architecture"), T(PULL_REF, "Public surface"), T(PUSH_REF, "Owns")],
    match_mode="all_of", tags=["multi_section_synthesis"])

add("dev", "multi_section_synthesis",
    "Combine the Ledger canon's L1 tokenizer-correctness step with its Unicode-normalization section: what must ship before the larger FTS architecture lands?",
    [T(LEDGER_CANON, "L1 — Fix live tokenizer correctness"), T(LEDGER_CANON, "8.1 Unicode normalization")],
    match_mode="all_of", tags=["multi_section_synthesis"])

add("dev", "multi_section_synthesis",
    "Combine the four-roots table with the uninstall bullet list: what happens to the `data` root on a plain uninstall?",
    [T(ROOTS, "The four roots"), T(ROOTS, "How install / update / rollback / uninstall use the roots")],
    match_mode="all_of", tags=["multi_section_synthesis"])

add("heldout", "multi_section_synthesis",
    "According to both README.md and docs/product/README.md, what does the 'Guide' (now Ledger) axis do?",
    [T(ROOT_README, "Six axes"), T(PRODUCT, "Six axes")],
    match_mode="all_of", tags=["multi_section_synthesis", "naming_drift"],
    notes="Both generated/handwritten docs still say 'Guide' rather than 'Ledger' at the time of this corpus snapshot; a correct system should treat them as the same renamed subsystem per agent-rules.md.")

add("heldout", "multi_section_synthesis",
    "Combine Ledger canon invariant 9 with its production-reachability section: what must be true before Ledger FTS can be called 'active'?",
    [T(LEDGER_CANON, "4. Locked invariants"), T(LEDGER_CANON, "9.4 Production reachability")],
    match_mode="all_of", tags=["multi_section_synthesis"])

add("heldout", "multi_section_synthesis",
    "Combine the cross-provider budget table and the process-plane table in docs/architecture/runtime-truth.md: which plane enforces the global ceiling, and what does a resolver-backed block consume?",
    [T(ARCHITECTURE, "Cross-provider budget model"), T(ARCHITECTURE, "Process plane separation (MBR-108)")],
    match_mode="all_of", tags=["multi_section_synthesis"])

# ===========================================================================
# 17. stale_relocation  (6: 2 / 2 / 2)
# ===========================================================================
add("train", "stale_relocation",
    "Where is docs/subsystems/spine.md?",
    [T(LEDGER_REF, "Definition of Done", "`docs/subsystems/spine.md` is retired in favor of this file.")],
    status="relocation", tags=["stale_relocation"],
    notes="docs/subsystems/spine.md does not exist in this checkout; ledger.md itself records the retirement/relocation. Correct behavior is typed relocation to ledger.md, not a literal spine.md hit.")

add("train", "stale_relocation",
    "What does README.md's six-axes table say the 'Guide' axis does?",
    [T(ROOT_README, "Six axes", "Navigates indexed document sections with hash-bound references"),
     T(LEDGER_REF, "Purpose", "Where in the documents is the relevant material?")],
    match_mode="any_of", tags=["stale_relocation", "naming_drift"],
    notes="README.md has not yet been through the Guide->Ledger rename cutover (agent-rules.md: 'Guide is retired; guide-named code/paths are pending rename, not a second name'). A correct system should recognize the still-live 'Guide' text as the same subsystem as ledger.md, not two separate concepts.")

add("dev", "stale_relocation",
    "What document did 04-GUIDE-MARKDOWN-INDEXING-REVIEW.md get superseded by?",
    [T(LEDGER_CANON, "Ledger Markdown Indexing and Document Navigation Canon",
       "Supersedes:** `04-GUIDE-MARKDOWN-INDEXING-REVIEW.md` in full")],
    status="relocation", tags=["stale_relocation"],
    notes="04-GUIDE-MARKDOWN-INDEXING-REVIEW.md is not present in this checkout; the canon itself is the current authoritative replacement and names its own supersession.")

add("dev", "stale_relocation",
    "Where does the guide::ledger session module live now?",
    [T(LEDGER_CANON, "2.2 Existing session-ledger collision", "ledger::session_projection")],
    status="relocation", tags=["stale_relocation"],
    notes="The canon documents a planned rename of guide::ledger to ledger::session_projection to avoid a ledger::ledger collision; a correct system must not fabricate a currently-shipping ledger::ledger module.")

add("heldout", "stale_relocation",
    "Can Ledger candidates already participate in the Membrane planner?",
    [T(LEDGER_REF, "Definition of Done",
       "[ ] Ledger candidates can participate in the Membrane planner rather than remaining permanently shadow-only.")],
    status="relocation", tags=["stale_relocation"],
    notes="This Definition-of-Done checkbox is unchecked ('[ ]'): the capability is documented as not-yet-landed. Correct behavior surfaces the unchecked item rather than implying the capability ships.")

add("heldout", "stale_relocation",
    "Where is the 'Markdown Doc Spine' now that the 2026-07-30 changelog entry described its absorption?",
    [T(LEDGER_CANON, None, "**Historical names:** Spine / Markdown Doc Spine / Guide")],
    status="relocation", tags=["stale_relocation"],
    plausible_distractors=[CHANGELOG],
    notes="CHANGELOG.md's 2026-07-30 entry ('RMS + Markdown Doc Spine absorption') is a historical snapshot; the current authoritative identity for that material is the Ledger canon, which lists 'Spine / Markdown Doc Spine / Guide' as historical names for Ledger.")

if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("--repo-root", required=True)
    parser.add_argument("--out-dir", required=True)
    args = parser.parse_args()

    repo_root = args.repo_root
    out_dir = args.out_dir
    os.makedirs(out_dir, exist_ok=True)

    errors = []
    doc_cache = {}

    def load(doc):
        if doc not in doc_cache:
            p = os.path.join(repo_root, doc)
            if not os.path.isfile(p):
                doc_cache[doc] = None
            else:
                with open(p, encoding="utf-8") as f:
                    doc_cache[doc] = f.read()
        return doc_cache[doc]

    for case in CASES:
        for t in case["expected"]["targets"]:
            doc = t["document"]
            text = load(doc)
            if text is None:
                errors.append(f"{case['id']}: missing document {doc}")
                continue
            if t["heading"]:
                # heading may include markdown backticks/punctuation stripped from source;
                # just require the heading text appears verbatim somewhere as a heading line.
                pattern = re.compile(r"^#{1,6}\s*" + re.escape(t["heading"]).replace(r"\ ", r"\s*"), re.MULTILINE)
                # Fallback: loose containment check on any heading line.
                heading_lines = [l for l in text.splitlines() if l.lstrip().startswith("#")]
                found = any(t["heading"] in l for l in heading_lines)
                if not found:
                    errors.append(f"{case['id']}: heading not found in {doc}: {t['heading']!r}")
            if t["quote"]:
                if t["quote"] not in text:
                    errors.append(f"{case['id']}: quote not found in {doc}: {t['quote']!r}")
        for doc in case["expected"].get("plausible_distractors", []):
            if load(doc) is None:
                errors.append(f"{case['id']}: missing distractor document {doc}")

    if errors:
        sys.stderr.write("VALIDATION ERRORS:\n" + "\n".join(errors) + "\n")
        sys.exit(1)

    # Referenced documents -> manifest with sha256
    referenced = set()
    for case in CASES:
        for t in case["expected"]["targets"]:
            referenced.add(t["document"])
        for d in case["expected"].get("plausible_distractors", []):
            referenced.add(d)

    manifest_docs = []
    for doc in sorted(referenced):
        text = doc_cache[doc]
        h = hashlib.sha256(text.encode("utf-8")).hexdigest()
        corpus = (
            "research-papers-secondary"
            if doc.startswith("docs/research/legacy-source-corpus/papers/")
            else "historical-reference-secondary"
            if doc.startswith("docs/research/legacy-source-corpus/derived-architecture/")
            else "membrane-docs-primary"
        )
        manifest_docs.append({
            "path": doc,
            "sha256": h,
            "bytes": len(text.encode("utf-8")),
            "corpus": corpus,
        })

    splits = {"train": [], "dev": [], "heldout": []}
    for case in CASES:
        splits[case["split"]].append(case)

    for split_name, rows in splits.items():
        out_path = os.path.join(out_dir, f"{split_name}.jsonl")
        with open(out_path, "w", encoding="utf-8") as f:
            for row in rows:
                f.write(json.dumps(row, ensure_ascii=False) + "\n")

    case_type_counts = {}
    for case in CASES:
        case_type_counts.setdefault(case["case_type"], {"train": 0, "dev": 0, "heldout": 0})
        case_type_counts[case["case_type"]][case["split"]] += 1

    manifest = {
        "schemaVersion": "ledger_eval_corpus.manifest.v1",
        "corpusId": "ledger-eval-v1",
        "totalCases": len(CASES),
        "splitCounts": {k: len(v) for k, v in splits.items()},
        "caseTypeCounts": case_type_counts,
        "documents": manifest_docs,
    }
    with open(os.path.join(out_dir, "manifest.json"), "w", encoding="utf-8") as f:
        json.dump(manifest, f, indent=2, ensure_ascii=False)
        f.write("\n")

    print(f"OK: {len(CASES)} cases, {len(referenced)} referenced documents, 0 validation errors.")
    print(json.dumps({k: len(v) for k, v in splits.items()}, indent=2))
    print(json.dumps(case_type_counts, indent=2))
