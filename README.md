# Adapt

> **TL;DR:** Adapt makes valid corrections stick across Codex/Claude sessions: it promotes only safe, scoped, evidence-linked guidance into reversible MemRight rules instead of treating every transcript line as policy.

AI assistants often repeat same mistakes because useful corrections disappear with session. Adapt
converts repeated, durable user guidance into a small preference layer future agents can recall.

It does **not** retrain model & does not save private chain-of-thought.

## How it works

```text
local Codex + Claude transcripts
              │
              ▼
 parse → redact → prefilter → extract
              │
              ▼
 dedupe → synthesize → authority checks
              │
              ▼
 immutable review manifest
              │
              ▼
 conformance gate → transactional MemRight apply
              │
              ▼
 scoped recall in future sessions
```

Adapt learns rules such as:

- “Run focused tests before broad build.”
- “This repository uses one updater trust root.”
- “Use this operational sequence only inside project X.”

It rejects transient facts such as “service is down today,” unsafe permission expansion, insecure
coding taste, obsolete workflows & instructions copied from repository/tool/assistant text.

## Rule types

Rules are typed so recall does not turn every lesson into global command:

| Type | Meaning |
|---|---|
| `standing_preference` | broad durable preference; eligible for bounded always-on core |
| `locked_decision` | current decision, only inside declared scope |
| `operational_playbook` | procedure recalled when task/scope matches |
| `episodic_fact` | supporting context, not standing instruction |
| `unclassified` | legacy/review state |

Default manual rule type is `operational_playbook`, not global preference.

## Preference record

Canonical record carries:

- stable content-derived ID & schema version;
- rule, controlled category & scope;
- structured scope dimensions such as path/platform/tool;
- record type & authority effect;
- confidence, evidence count & review flag;
- source session IDs & retrieval aliases;
- accepted/lifecycle state;
- creation/update/reverification timestamps;
- machine attribution plus optional machine-only narrowing.

Rules can be active, retired, deprecated or superseded. Reverification is explicit presence/count,
not arbitrary age-decay score—stable preference does not become false merely because it is old.

## Authority boundary

Only authenticated user-origin evidence can create durable preference authority.

Adapt deterministically quarantines:

- assistant-authored narration;
- tool output or repository text echoed into transcript;
- permission/approval expansion;
- security weakening;
- conflicts with current `AGENTS.md`, `CLAUDE.md` or workspace rules;
- contradictions with active stored rule;
- forbidden or mismatched scope;
- unknown categories, duplicate IDs, empty/short rules & transient environment claims.

Origin tags are checked first. Lexical fallback still scans evidence for prompt-injection-shaped
content if origin is missing or mislabeled.

This prevents a malicious repository file from teaching agent a permanent instruction merely
because file appeared in session transcript.

## Review manifests

Mining does not write candidates directly on current multiwriter installations.

Adapt emits JSON manifest containing:

- exact source session identities;
- transcript/source hashes;
- candidate payload SHA-256;
- rule type, scope, authority effect & evidence links;
- explicit `accepted`, `rejected` or `pending` decision.

Apply refuses:

- pending records;
- edited payload whose hash no longer matches;
- changed canonical rule pool;
- source sessions from another installation;
- duplicated/out-of-manifest evidence;
- authority-quarantined candidates.

This makes review artifact immutable between “what was approved” & “what was written.”

## Multi-machine safety

Each installation has opaque identity. Session IDs become installation-qualified, so identical
local session names on two machines cannot collide.

Before apply, Adapt binds manifest to:

- installation ID;
- exact canonical MemRight rule-pool hash;
- exact source-session namespace;
- conformance receipt;
- current session inventory.

Incremental multiwriter runner creates isolated work directory, mines pending manifest, adjudicates,
revalidates conformance, applies through one manifest-only path & writes content-hashed phase
receipts/summary. If input drifts, it fails closed & does not advance session state.

## Transaction, resume & rollback

- run journal checkpoints discovery, extraction & synthesis;
- safe resume reuses cached stages only when session identity still matches;
- interrupted/stale run never silently marks sessions learned;
- multiwriter persistence is all-or-nothing through resident service;
- legacy apply captures SQLite-safe backup plus state/rules/core snapshots first;
- rollback deletes only recorded IDs, restores snapshots & runs `PRAGMA integrity_check`;
- no force flag bypasses failed integrity proof.

## Delivery through MemRight

Accepted records are stored in MemRight’s local SQLite database. Future prompt-time recall matches:

- repository/workspace scope;
- structured dimensions;
- rule text;
- safe retrieval aliases;
- semantic/keyword relevance;
- lifecycle & authority type.

Only small set of root-scoped standing preferences can be compiled into bounded always-on core.
Project/tool/playbook knowledge remains recall-gated, preventing preference layer from becoming
another giant prompt.

## What makes it different

Adapt is not transcript summarization. Its concrete advantage is promotion pipeline:

- **Corrections become data:** durable guidance is typed, scoped & source-linked.
- **User-origin authority:** repository/model/tool content cannot self-promote into rules.
- **Manifest immutability:** reviewed bytes are exact bytes applied.
- **Cross-machine binding:** installation/session/pool identity blocks wrong-writer merges.
- **Rule lifecycle:** decisions can be reverified, narrowed, retired, superseded or rolled back.
- **Recall aliases without authority expansion:** vocabulary aids retrieval but never changes rule.
- **Bounded always-on core:** only broad standing preferences load globally; rest stays on demand.
- **Dry-run first:** smoke, manifest & default CLI paths preview before live write.
- **Outcome accounting:** discovery, accepted/rejected/persisted/failure counts remain separated by
  client instead of calling every mined candidate “learning.”

The moat is safe conversion from messy human corrections into portable agent behavior without
letting transcript content become policy by accident.

## Main commands

Adapt currently runs as workspace-integrated Python tooling:

```sh
python3 adapt.py --smoke
python3 adapt.py --incremental --manifest pending.json
python3 adapt.py --apply-from-manifest resolved.json
python3 adapt.py --compile-core path/to/core.json

python3 adapt.py \
  --add-rule "Always run focused tests before broad builds." \
  --category verification
```

Writes are opt-in with `--apply`; smoke & manifest generation remain dry-run. External model lanes
require explicit `--allow-external-lane`, & transcript text is redacted/scanned before leaving local
lane.

Workspace installations use `run_incremental_multiwriter.py` for receipt-gated incremental mining.

## Privacy & trust

- local transcripts are source; secret scanner drops unsafe content;
- only redacted batches may enter allowed external lane;
- no private chain-of-thought is retained;
- provenance uses hashes & qualified source IDs;
- MemRight remains local source of truth;
- audit/receipt artifacts avoid raw transcript content where possible;
- authority files remain source of truth & are never rewritten by mined preference.

## Current scope

Adapt ships transcript parsing, extraction/synthesis lanes, controlled taxonomy, origin/authority
quarantine, contradiction checks, typed records, immutable manifests, multiwriter conformance,
journaling, rollback, core compilation & MemRight persistence.

Current limits:

- standalone checkout depends on parent workspace memory/session modules & installed MemRight;
- model-assisted extraction still requires available configured lane;
- lexical contradiction detection catches direct polarity conflicts, not every semantic conflict;
- final quality depends on review/adjudication policy & source transcript quality;
- only standing preferences qualify for always-on core; other records require relevant recall.
