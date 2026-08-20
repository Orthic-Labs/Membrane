# Cortex — Document Lifecycle

Supersession markers, structured lifecycle frontmatter, archive globs, and authority resolution.
Extracted from `skills/cortex/SKILL.md` to keep the operational contract readable. **This is normative**, not
background reading — Phase 1 and Phase 4 both depend on it.

## Historical-document lifecycle

Superseded documents remain in the map as provenance, but they are not current claims. Mark a wholly
retired document at the very top with exactly one of these two-line forms:

```markdown
> Superseded by `repo/relative/source.ext` on YYYY-MM-DD.
> Retained as historical context; do not treat as current.
```

```markdown
> Superseded on YYYY-MM-DD.
> Historical record of a retired external surface; the live surface is audited separately outside this repository.
```

The first target must be an indexed repo-relative regular file whose physical path remains inside
the repo; absolute paths, `../` escapes, symlink/junction escapes, directories, and unindexed targets
do not retire the document and are emitted in `stale.json.invalidSupersessionMarkers`.
When a recognized top-level supersession first line has a missing or malformed required historical
note, the document likewise stays current and emits `historical-note-invalid` in that list.
Cortex records `doc.lifecycle`, retains a `supersedes` relationship when the canonical source is
inside the repo, and excludes superseded content from live claims, stale-reference findings, Phase-2
queues, generated current docs, and task briefs. Paths matching `archiveGlobs` (defaults:
`docs/archive/`, `docs/history/`) receive the same live-input exclusion with `status: archived`.

## Prefer structured frontmatter; treat the banner as display

A markdown banner is a convention that is easy to get subtly wrong, and a malformed one only
surfaces in `stale.json.invalidSupersessionMarkers`. Where a document declares its own lifecycle,
read it from frontmatter and render the banner for humans:

```yaml
cortex:
  document_id: adr-auth-004
  type: decision          # decision | plan | reference | runbook | historical
  status: accepted        # draft | accepted | superseded | rejected
  effective_from: 2026-06-12
  supersedes: [adr-auth-002]
  scope: { deployable_units: [api, desktop], branches: [main] }
  canonical_for: [authentication, token-storage]
```

## Newer does not mean more authoritative

Filename dates, frontmatter dates and recent commits are evidence of chronology, not of authority —
a newer scratch note does not supersede a ratified ADR, a typo fix does not make an old plan
current, and a branch-specific doc may not apply to the deployable unit under inspection. Resolve
authority in this order:

1. explicit status and supersession
2. scope match
3. canonical topic ownership
4. effective revision/date
5. branch/release
6. chronology — last resort only

Chronology alone never settles a divergence.

## Scope of a banner

Use a whole-document banner only when the entire document is historical. If one row, section, or
claim is stale, update that claim or add an inline canonical-source pointer; an inline
`Superseded by` note never retires the whole document. A deployed site whose source moved to another
repository is outside this repository's Cortex scope; map that owning repo separately, run
`/seo audit <url>` for deployed crawl/index/content/schema/CWV evidence, and run `/audit-visual <url>`
for rendered UI/UX evidence. Neither live-URL pass replaces the owning repository's `/audit`.
