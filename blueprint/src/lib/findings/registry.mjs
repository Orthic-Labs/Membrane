// Finding registry — the closed catalogue of deterministic, graph-derived
// diagnostics Blueprint can prove without running a compiler, a test or a
// build.
//
// Every rule declares:
//   precisionFloor — the weakest evidence tier allowed to emit it. A rule is
//                    never emitted from a weaker tier; it is omitted instead.
//   class          — "block" means the finding is provable and a consumer may
//                    refuse an expensive check on it. "advisory" means probable.
//
// IDs are stable and public: they appear in SARIF output, in baselines and in
// agent-facing text. Retiring one means marking it retired, never reusing it.

export const FINDING_RULES = Object.freeze({
  BP001: Object.freeze({
    id: "BP001",
    name: "import-binding-not-exported",
    severity: "error",
    class: "block",
    precisionFloor: "AST",
    description: "An imported name is not exported by the module the specifier resolves to.",
    remediation: "Import a name the target module exports, or export the missing name from it.",
  }),
  BP002: Object.freeze({
    id: "BP002",
    name: "module-not-found",
    severity: "error",
    class: "block",
    precisionFloor: "AST",
    description: "A repository-relative import specifier resolves to no file in the repository.",
    remediation: "Correct the specifier, or create the module it names.",
  }),
  BP003: Object.freeze({
    id: "BP003",
    name: "reexport-binding-not-exported",
    severity: "error",
    class: "block",
    precisionFloor: "AST",
    description: "A re-export names a binding the target module does not export, breaking every consumer of this barrel.",
    remediation: "Re-export a name the target module exports, or export the missing name from it.",
  }),
});

export const FINDING_RULE_IDS = Object.freeze(Object.keys(FINDING_RULES).sort());

export function findingRule(ruleId) {
  return FINDING_RULES[ruleId] ?? null;
}

// Every reason a request could not be judged. Emitted as omissions so a clean
// run is distinguishable from an unexamined one — "no findings" must never be
// able to mean "nothing was checked".
export const OMISSION_REASONS = Object.freeze([
  "unsupported_language",       // extension outside the JS/TS family
  "parse_failed",               // grammar could not parse the file
  "open_export_surface",        // target module's exports are not enumerable
  "package_specifier",          // bare specifier — outside repository scope
  "outside_scanned_set",        // target exists on disk but was not scanned
  "star_depth_exceeded",        // `export *` chain deeper than the bound
  "star_cycle",                 // `export *` cycle
  "resolution_ambiguous",       // multiple candidates match — fail-closed
]);
