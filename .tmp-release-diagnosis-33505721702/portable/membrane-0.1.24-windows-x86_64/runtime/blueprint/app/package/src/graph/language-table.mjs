// D21: immutable declarative language table (S-18 contract). The generic
// walker uses only explicit fields/child selectors/query captures from the
// table — it never guesses a name from arbitrary text.

export function defineLanguageTable(value) {
  const table = {
    id: String(value.id),
    extensions: Object.freeze([...value.extensions].sort()),
    grammarFile: String(value.grammarFile),
    factProfile: value.factProfile ?? "code",
    declarations: Object.freeze(value.declarations ?? []),
    functions: Object.freeze(value.functions ?? []),
    classes: Object.freeze(value.classes ?? []),
    imports: Object.freeze(value.imports ?? []),
    calls: Object.freeze(value.calls ?? []),
    relationships: Object.freeze(value.relationships ?? []),
    comments: Object.freeze(value.comments ?? []),
    tests: Object.freeze(value.tests ?? []),
    capabilities: Object.freeze(value.capabilities ?? {}),
  };
  return Object.freeze(table);
}
