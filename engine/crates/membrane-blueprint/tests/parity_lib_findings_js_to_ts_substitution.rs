// Lane LIB5 (r5 windows closure): proves findings.rs's own resolve_candidates
// path now performs the TypeScript `.js` -> `.ts` extension substitution
// (via lib_findings_specifier::candidate_paths) that its previous inline
// implementation lacked. Before the fix, an import specifier ending in
// `.js` whose only on-disk match was a `.ts` file with the same stem
// resolved to zero candidates (the old code appended extensions onto the
// literal `.js`-suffixed base, e.g. `src/b.js.ts`, never `src/b.ts`), so the
// import falsely reported BP002 ("resolves to no file in the repository").

use membrane_blueprint::api::{BlueprintRequest, Bounds, Operation};
use membrane_blueprint::findings::execute_findings;
use membrane_blueprint::store::Generation;
use serde_json::json;
use std::path::Path;

fn generation_importing_js_that_only_exists_as_ts() -> Generation {
    serde_json::from_value(json!({
        "manifest": {"generationId": "g1"},
        "nodes": [
          {"kind":"file","path":"src/a.ts","evidence":[{"contentHash":"sha256:a","moduleSurface":{
              "path":"src/a.ts","parseStatus":"ok","exports":[],"starReexports":[],
              "requests":[{"kind":"import","name":"thing","specifier":"./b.js","line":1}],"open":[]
          }}]},
          {"kind":"file","path":"src/b.ts","evidence":[{"contentHash":"sha256:b","moduleSurface":{
              "path":"src/b.ts","parseStatus":"ok","exports":[{"name":"thing"}],"starReexports":[],"requests":[],"open":[]
          }}]}
        ], "edges": [], "fileReports": []
    }))
    .unwrap()
}

#[test]
fn js_specifier_resolves_to_sibling_ts_file_via_findings_dispatch() {
    let generation = generation_importing_js_that_only_exists_as_ts();
    let request = BlueprintRequest::new("r1", Operation::FindingsGet, ".");
    let context = request.validate(Bounds::default()).unwrap();
    let out = execute_findings(&generation, &request, &context, Path::new("target/findings-js-ts-test-state")).unwrap();
    // `./b.js` resolves to `src/b.ts` (which exports `thing`), so no BP001 or
    // BP002 finding is raised and no `resolution_ambiguous` /
    // `package_specifier` omission is emitted for this import.
    let findings = out["findings"].as_array().cloned().unwrap_or_default();
    assert!(findings.is_empty(), "expected no findings once .js resolves to the sibling .ts file, got {findings:?}");
    let omissions = out["omissions"].as_array().cloned().unwrap_or_default();
    assert!(
        !omissions.iter().any(|o| o["path"] == "src/a.ts" && o["specifier"] == "./b.js"),
        "expected no resolution omission for ./b.js once it resolves to src/b.ts, got {omissions:?}"
    );
}
