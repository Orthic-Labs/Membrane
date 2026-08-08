# Vault review export

`crypt vault-export` creates a deterministic, versioned review queue from Crypt's authoritative store. Export is read-only; SQLite remains authoritative, but reviewers need only generated JSON or Markdown.

```sh
crypt --db /path/to/memory.db vault-export --output vault-review.json
crypt --db /path/to/memory.db vault-export --output vault-review.md --format markdown
```

Content is excluded by default. Add `--include-content` only for an explicitly approved local review artifact:

```sh
crypt --db /path/to/memory.db vault-export --output private-review.md --format markdown --include-content
```

## Contract

- `schemaVersion` is `1`; `kind` is `crypt.vault-review`.
- Queue order is `reviewAfterMs` ascending with null last, protected priority first on equal dates, then bytewise memory ID.
- Each row leads with content-free identity, hash, scope, lifecycle, expiry, review, supersession, priority, confidence, provenance, authority, event attribution, & verified review evidence.
- Supersession is represented in both directions through `lifecycle.supersededBy` & `supersedes`.
- Contradictions remain explicit `reviewEvidence` outcomes.
- Missing authoritative approval & context-pack-reference tables are reported as unavailable; values are never inferred.
- JSON & Markdown contain equivalent metadata. Markdown places metadata before optional content.
- A repeated export from unchanged state is byte-identical.

Output parent must already exist; parent traversal & symlink path components are rejected. Existing symlink or non-file targets are rejected. Writes use a same-directory temporary file plus atomic rename; identical output is left untouched.
