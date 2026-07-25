//! L3 router: prepare a set of files for context injection.
//!
//! Parity target: workspace `prep-context.py` (real behavior, not the concept
//! doc). This module writes prepared files under `out_dir` and returns a JSON
//! manifest whose entry shape is branch-specific.

use crate::{compress, skel};
use serde::Serialize;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize)]
pub struct PrepEntry {
    pub orig: String,
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prepared: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub before_bytes: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub after_bytes: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub before_tok: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub after_tok: Option<u64>,
}

fn is_code_ext(path: &Path) -> bool {
    let ext = path
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    matches!(
        ext.as_str(),
        "js" | "jsx"
            | "mjs"
            | "cjs"
            | "ts"
            | "tsx"
            | "py"
            | "rs"
            | "go"
            | "java"
            | "c"
            | "h"
            | "cc"
            | "cpp"
            | "hpp"
            | "cs"
            | "rb"
            | "php"
            | "swift"
            | "kt"
            | "kts"
            | "scala"
            | "sh"
            | "bash"
    )
}

fn base_name(path: &Path) -> String {
    path.file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("x")
        .to_string()
}

fn estimate_tokens(text: &str) -> u64 {
    // This is an estimate only; parity with Python LLMLingua happens behind
    // `llmlingua-onnx` and a separate 4b test.
    text.split_whitespace().count() as u64
}

/// `(verb, before, after, meta)` transform_log rows for a manifest — shared by the CLI and serve
/// logging paths. skel rows carry bytes; compress rows carry token counts (meta marks the unit so
/// `metrics` never mixes units within a verb); copy/missing carry 0s.
pub fn transform_rows(manifest: &[PrepEntry]) -> Vec<(String, usize, usize, Option<&'static str>)> {
    manifest
        .iter()
        .map(|e| {
            let (before, after, meta) = match e.kind.as_str() {
                "skel" | "skel-fallback-copy" => (
                    e.before_bytes.unwrap_or(0) as usize,
                    e.after_bytes.unwrap_or(0) as usize,
                    None,
                ),
                "compress" => (
                    e.before_tok.unwrap_or(0) as usize,
                    e.after_tok.unwrap_or(0) as usize,
                    Some("unit=tok"),
                ),
                _ => (0, 0, None),
            };
            (format!("prep:{}", e.kind), before, after, meta)
        })
        .collect()
}

/// Prepare `files` under `out_dir`, returning a branch-specific manifest.
pub fn prep_files(
    out_dir: &Path,
    files: &[PathBuf],
    rate: f32,
    min_bytes: usize,
) -> Vec<PrepEntry> {
    let _ = std::fs::create_dir_all(out_dir);
    let mut out = Vec::new();

    for orig_path in files {
        let orig_str = orig_path.to_string_lossy().to_string();

        // Branch 1: missing
        if !orig_path.is_file() {
            out.push(PrepEntry {
                orig: orig_str,
                kind: "missing".into(),
                prepared: None,
                before_bytes: None,
                after_bytes: None,
                before_tok: None,
                after_tok: None,
            });
            continue;
        }

        let bytes = std::fs::read(orig_path).unwrap_or_default();
        let before_bytes = bytes.len() as u64;

        // Branch 2: tiny/missing content -> copy (tiny beats code ext)
        if bytes.len() < min_bytes {
            let name = base_name(orig_path);
            let prepared_path = out_dir.join(&name);
            let _ = std::fs::write(&prepared_path, &bytes);
            out.push(PrepEntry {
                orig: orig_str,
                kind: "copy".into(),
                prepared: Some(prepared_path.to_string_lossy().to_string()),
                before_bytes: None,
                after_bytes: None,
                before_tok: None,
                after_tok: None,
            });
            continue;
        }

        // Load text once for remaining branches.
        let src = String::from_utf8_lossy(&bytes).to_string();

        // Branch 3: code ext -> skel
        if is_code_ext(orig_path) {
            let name = format!("{}.skel", base_name(orig_path));
            let prepared_path = out_dir.join(&name);
            let sk = skel::skeletonize(orig_path, &src);
            let (kind, payload) = if sk.trim().is_empty() || sk == src {
                ("skel-fallback-copy".to_string(), src)
            } else {
                ("skel".to_string(), sk)
            };
            let _ = std::fs::write(&prepared_path, &payload);
            out.push(PrepEntry {
                orig: orig_str,
                kind,
                prepared: Some(prepared_path.to_string_lossy().to_string()),
                before_bytes: Some(before_bytes),
                after_bytes: Some(payload.len() as u64),
                before_tok: None,
                after_tok: None,
            });
            continue;
        }

        // Branch 4: everything else -> compress
        let ext = orig_path.extension().and_then(|s| s.to_str());
        let name = match ext {
            Some(ext) if !ext.is_empty() => format!("{}.min.{ext}", base_name(orig_path)),
            _ => format!("{}.min.txt", base_name(orig_path)),
        };
        let prepared_path = out_dir.join(&name);
        let before_tok = estimate_tokens(&src);
        let compressed = compress::compress(&src, rate);
        let after_tok = estimate_tokens(&compressed);
        let _ = std::fs::write(&prepared_path, &compressed);
        out.push(PrepEntry {
            orig: orig_str,
            kind: "compress".into(),
            prepared: Some(prepared_path.to_string_lossy().to_string()),
            before_bytes: None,
            after_bytes: None,
            before_tok: Some(before_tok),
            after_tok: Some(after_tok),
        });
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(feature = "llmlingua-onnx")]
    use std::collections::HashSet;

    #[test]
    fn prep_routes_and_manifest() {
        let tmp = tempfile::tempdir().unwrap();
        let out_dir = tmp.path().join("out");
        std::fs::create_dir_all(&out_dir).unwrap();

        // missing
        let missing = tmp.path().join("nope.rs");

        // tiny (copy branch wins even though it's code)
        let tiny = tmp.path().join("tiny.rs");
        std::fs::write(&tiny, "fn x(){}").unwrap();

        // big (skel branch)
        let big = tmp.path().join("big.rs");
        let big_body = "fn a(x:i32)->i32 { x+1 }\n".repeat(200);
        std::fs::write(&big, &big_body).unwrap();

        // prose (compress branch)
        let notes = tmp.path().join("notes.md");
        std::fs::write(&notes, "Hello world.\n".repeat(200)).unwrap();

        let files = vec![missing.clone(), tiny.clone(), big.clone(), notes.clone()];
        let manifest = prep_files(&out_dir, &files, 0.5, 50);
        assert_eq!(manifest.len(), 4);

        // missing: orig+kind only
        assert_eq!(manifest[0].kind, "missing");
        assert!(manifest[0].prepared.is_none());
        assert!(manifest[0].before_bytes.is_none());
        assert!(manifest[0].before_tok.is_none());

        // tiny copy: prepared exists, no size fields
        assert_eq!(manifest[1].kind, "copy");
        let tiny_prepared = PathBuf::from(manifest[1].prepared.clone().unwrap());
        assert_eq!(
            tiny_prepared.file_name().and_then(|s| s.to_str()),
            Some("tiny.rs")
        );
        assert!(manifest[1].before_bytes.is_none());
        assert!(manifest[1].before_tok.is_none());

        // big skel: .skel file and byte fields
        assert!(manifest[2].kind == "skel" || manifest[2].kind == "skel-fallback-copy");
        let big_prepared = PathBuf::from(manifest[2].prepared.clone().unwrap());
        assert_eq!(
            big_prepared.file_name().and_then(|s| s.to_str()),
            Some("big.rs.skel")
        );
        assert!(manifest[2].before_bytes.is_some());
        assert!(manifest[2].after_bytes.is_some());

        // notes compress: .min.md file and token fields
        assert_eq!(manifest[3].kind, "compress");
        let notes_prepared = PathBuf::from(manifest[3].prepared.clone().unwrap());
        assert_eq!(
            notes_prepared.file_name().and_then(|s| s.to_str()),
            Some("notes.md.min.md")
        );
        assert!(manifest[3].before_tok.is_some());
        assert!(manifest[3].after_tok.is_some());
    }

    /// Task 4b parity (per `docs/plans/2026-07-01-context-engine-unification.md`):
    /// the prep `compress` branch's `.min.<ext>` file content and the manifest's
    /// `before_tok`/`after_tok` numbers only match Python `compress.py` once the
    /// LLMLingua-ONNX path lands. Gated on `--features llmlingua-onnx` (the
    /// heuristic `--no-onnx` fallback does NOT byte-match `compress.py`).
    ///
    /// Same 5 fixtures as `compress::tests::compress_matches_python_jaccard`,
    /// same threshold (0.90 word-Jaccard), same skip-when-assets-absent policy.
    #[cfg(feature = "llmlingua-onnx")]
    #[test]
    fn prep_compress_branch_matches_python_jaccard() {
        fn word_set(s: &str) -> HashSet<String> {
            s.split(|c: char| !c.is_alphanumeric())
                .filter(|p| !p.is_empty())
                .map(|p| p.to_lowercase())
                .collect()
        }

        // Mirror the asset-present check from compress.rs (avoids pulling the
        // full llmlingua-onnx module into scope just for one function call).
        fn assets_present() -> bool {
            let manifest = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
            let default_dir = manifest.join("assets").join("llmlingua");
            let model = default_dir.join("model.onnx");
            let tok = default_dir.join("tokenizer.json");
            model.is_file() && tok.is_file()
        }

        if !assets_present() {
            eprintln!("prep_compress_branch_matches_python_jaccard: assets not present, skipping");
            return;
        }

        const FIXTURES: &[(&str, &str)] = &[
            (
                "01_meeting.txt",
                include_str!("../../../spike/fixtures/01_meeting.txt"),
            ),
            (
                "02_readme.txt",
                include_str!("../../../spike/fixtures/02_readme.txt"),
            ),
            (
                "03_incident.txt",
                include_str!("../../../spike/fixtures/03_incident.txt"),
            ),
            (
                "04_doc.txt",
                include_str!("../../../spike/fixtures/04_doc.txt"),
            ),
            (
                "05_plan.txt",
                include_str!("../../../spike/fixtures/05_plan.txt"),
            ),
        ];
        const THRESHOLD: f64 = 0.90;

        let golden_str = include_str!("../tests/llmlingua_golden.json");
        let golden: serde_json::Value =
            serde_json::from_str(golden_str).expect("golden JSON parses");

        let tmp = tempfile::tempdir().expect("tmpdir");
        let out_dir = tmp.path().join("out");
        std::fs::create_dir_all(&out_dir).expect("out_dir");

        for (name, text) in FIXTURES {
            // Write each fixture under a `.md` so prep routes it to the
            // compress branch (per `prep_routes_and_manifest`'s branch order:
            // missing → copy(tiny) → skel(code) → compress(otherwise)).
            let src_path = tmp.path().join(name);
            std::fs::write(&src_path, text).expect("write fixture");

            let files = vec![src_path.clone()];
            let manifest = prep_files(&out_dir, &files, 0.5, 50);

            assert_eq!(manifest.len(), 1, "fixture {name}: expected 1 entry");
            let entry = &manifest[0];
            assert_eq!(
                entry.kind, "compress",
                "fixture {name}: expected compress branch"
            );
            assert!(
                entry.before_tok.is_some(),
                "fixture {name}: missing before_tok"
            );
            assert!(
                entry.after_tok.is_some(),
                "fixture {name}: missing after_tok"
            );

            // Token-count sanity: after_tok must be smaller than before_tok. We only
            // assert `after < before` — the exact ratio depends on the metric
            // (LLMLingua keeps ~50% of *subword* tokens; `estimate_tokens`
            // counts whitespace-words, so the post-compression whitespace-word
            // count is naturally lower than 50% of the original).
            let before = entry.before_tok.unwrap();
            let after = entry.after_tok.unwrap();
            assert!(
                after < before && after > 0,
                "fixture {name}: after_tok ({after}) should be in (0, before_tok={before})"
            );

            // Read the .min.md file and compare its words to the Python golden.
            let prepared = entry.prepared.as_ref().expect("prepared path");
            let min_text = std::fs::read_to_string(prepared).expect("read .min file");
            let py_compressed = golden[*name]["compressed"]
                .as_str()
                .unwrap_or_else(|| panic!("missing golden entry for {name}"));
            let rust_words = word_set(&min_text);
            let py_words = word_set(py_compressed);
            let inter = rust_words.intersection(&py_words).count() as f64;
            let union = rust_words.union(&py_words).count() as f64;
            let score = if union == 0.0 { 1.0 } else { inter / union };
            assert!(
                score >= THRESHOLD,
                "fixture {name}: prep compress Jaccard {score:.4} < threshold {THRESHOLD}\n\
                 rust: {min_text:?}\n\
                 py:   {py_compressed:?}",
            );
        }
    }
}
