<img src=".github/banner.svg" alt="Morph — Corrections that stick across sessions." width="100%">

<sub>Package and CLI ids remain <code>adapt</code> — not re-keyed.</sub>

**Morph turns durable corrections into portable agent behavior: it promotes only safe, scoped, evidence-linked guidance into reversible MemRight rules instead of treating every transcript line as policy.**

[![License](https://img.shields.io/badge/license-source--available-df6428?style=flat-square&labelColor=111318)](LICENSE)

## What it is

AI assistants repeat the same mistakes because useful corrections disappear when a session ends.
Morph mines local Codex and Claude session transcripts for repeated, durable user guidance and
converts it into a small preference layer that future agents can recall. It does not retrain the
model and does not save private chain-of-thought.

| Surface | Role | Status |
|---|---|---|
| **Taste** | durable preferences → MemRight | ships (kernel fixes on P0/P1) |
| **Doctor** | `morph doctor` / `adapt doctor` → multiwriter conformance | ships; Blueprint/Beacon checks are **not-yet** |
| **Insights** | failure/waste mining | deferred — not a product yet |

Morph learns rules such as:

- "Always run focused tests before reporting a broad build complete."
- "This repository uses one shared updater trust root across every app."
- "Use this operational sequence only inside project X."

It rejects transient facts ("service is down today"), unsafe permission expansion, insecure coding
taste, obsolete workflows, and instructions copied from repository, tool, or assistant text.

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

Rules are typed so recall does not turn every lesson into a global command:

| Type | Meaning |
|---|---|
| `standing_preference` | broad durable preference; eligible for bounded always-on core |
| `locked_decision` | current decision, only inside declared scope |
| `operational_playbook` | procedure recalled when task/scope matches |
| `episodic_fact` | supporting context, not standing instruction |
| `unclassified` | legacy/review state |

Default manual rule type is `operational_playbook`, not global preference.

## Quick start

Morph runs as workspace-integrated Python tooling (CLI ids stay `adapt`/`morph`):

```sh
python3 adapt.py --smoke
python3 adapt.py --incremental --manifest pending.json
python3 adapt.py --apply-from-manifest resolved.json
python3 adapt.py --compile-core path/to/core.json

python3 adapt.py \
  --add-rule "Always run focused tests before reporting a broad build complete." \
  --category verification

# Doctor — multiwriter conformance only (Blueprint/Beacon = not-yet)
python3 morph.py doctor --scope
python3 morph.py doctor issue --out receipt.json
python3 adapt.py doctor validate --receipt receipt.json
```

Writes are opt-in with `--apply`; smoke and manifest generation stay dry-run. External model lanes
need explicit `--allow-external-lane`, and transcript text is redacted and scanned before leaving
the local lane.

Parent-workspace dependencies (MemRight port, session inventory, mirror) go through
`workspace_runtime.py` — see [docs/workspace-interface.md](docs/workspace-interface.md). Workspace
installations use `run_incremental_multiwriter.py` for receipt-gated incremental mining.

Install pinned test dependencies before running the full suite:

```sh
python3 -m pip install -r requirements-test.txt
python3 -m pytest -q
```

## Safety model

Only authenticated user-origin evidence can create durable preference authority. Morph
deterministically quarantines assistant-authored narration, echoed tool/repository output,
permission or approval expansion, security weakening, conflicts with the active `AGENTS.md`,
`CLAUDE.md`, or workspace rules, contradictions with an active stored rule, forbidden or mismatched
scope, and unknown categories, duplicate IDs, empty/short rules, or transient environment claims.
Origin tags are checked first; a lexical fallback still scans evidence for prompt-injection-shaped
content when origin is missing or mislabeled. This stops a malicious repository file from teaching
an agent a permanent instruction merely because it appeared in a transcript.

Mining never writes candidates directly. Morph emits a JSON review manifest with exact source
session identities, transcript/source hashes, candidate payload SHA-256, rule type, scope, authority
effect, evidence links, and an explicit `accepted`/`rejected`/`pending` decision. Apply refuses
pending records, an edited payload whose hash no longer matches, a changed canonical rule pool,
source sessions from another installation, out-of-manifest evidence, and authority-quarantined
candidates — so the reviewed artifact is exactly what gets written.

Each installation has an opaque identity; session IDs are installation-qualified so identical local
session names on two machines cannot collide. Before apply, Morph binds the manifest to the
installation ID, the exact canonical MemRight rule-pool hash, the source-session namespace, a
conformance receipt, and the current session inventory.

A run journal checkpoints discovery, extraction, and synthesis; safe resume reuses cached stages
only when session identity still matches, and an interrupted or stale run never silently marks
sessions learned. Multiwriter persistence is all-or-nothing through the resident service; legacy
apply captures a SQLite-safe backup plus state/rules/core snapshots first. Rollback deletes only
recorded IDs, restores snapshots, and runs `PRAGMA integrity_check` — no force flag bypasses a
failed integrity proof.

## Recall

Accepted records live in MemRight's local SQLite database. Prompt-time recall matches
repository/workspace scope, structured dimensions, rule text, safe retrieval aliases,
semantic/keyword relevance, and lifecycle/authority type. Only a small set of root-scoped standing
preferences compiles into the bounded always-on core; project/tool/playbook knowledge stays
recall-gated, so the preference layer never grows into another giant prompt.

## Status

Morph ships Taste transcript parsing, extraction/synthesis lanes, a controlled taxonomy,
origin/authority quarantine, contradiction checks, typed records, immutable manifests, multiwriter
conformance Doctor, journaling, rollback, core compilation, and MemRight persistence.

Implementation layout (P1 split): `taste.py` / `taste_apply.py` / `taste_mine.py` / `cli.py`, with
`adapt.py` as the compatible facade and `morph.py` as the display-name entry.

Current limits:

- a standalone checkout depends on parent-workspace memory/session modules and an installed
  MemRight (contract + stubs: `workspace_runtime.py`);
- model-assisted extraction still requires an available configured lane;
- lexical contradiction detection catches direct polarity conflicts, not every semantic conflict;
- final quality depends on review/adjudication policy and source transcript quality;
- only standing preferences qualify for the always-on core; other records need relevant recall;
- Doctor does not yet cover Blueprint or Beacon.

Source-available under the Orthic Labs Source Use License — see [LICENSE](LICENSE).

---

<sub><b><a href="https://orthic-labs.github.io">Orthic Labs</a></b> — local-first infrastructure for AI-assisted development.<br>
<a href="https://github.com/Orthic-Labs/Membrane">Membrane</a> · <a href="https://github.com/Orthic-Labs/Cortex">Cortex</a> · <a href="https://github.com/Orthic-Labs/Sentinel">Sentinel</a> · <a href="https://github.com/Orthic-Labs/Roundtable">Roundtable</a> · <a href="https://github.com/Orthic-Labs/Morph">Morph</a> · <a href="https://github.com/Orthic-Labs/CutRight">CutRight</a> · <a href="https://github.com/Orthic-Labs/claudecodeX">claudecodeX</a></sub>
