//! Native Rust port of `blueprint/src/lib/rules/baseline.mjs`.
//!
//! Lane LIB3 (r5 closure): no prior native equivalent found (grep of
//! `captureNamedBaseline`/`changedSlice`/`NAMED_BASELINES` across
//! membrane-blueprint/src and membrane-runtime/src produced no match).
//! Ported behavior: an in-process named-baseline store keyed by name,
//! capturing generation id + sorted fingerprints, plus `changed_slice`
//! (findings not present in a baseline's fingerprint set).

use std::collections::{HashMap, HashSet};
use std::sync::Mutex;
use std::sync::OnceLock;

#[derive(Debug, Clone)]
pub struct FindingRef {
    pub fingerprint: String,
    pub rule_id: Option<String>,
    pub path: Option<String>,
    pub name: Option<String>,
    pub specifier: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ChangedSlice {
    pub total: usize,
    pub new_count: usize,
    pub worsened_count: usize,
    pub findings: Vec<FindingRef>,
    pub retained_total: usize,
}

#[derive(Debug, Clone)]
pub struct BaselineSummary {
    pub name: String,
    pub generation_id: String,
    pub findings_fingerprints: Vec<String>,
    pub finding_count: usize,
    pub created_at: String,
}

#[derive(Debug, Clone)]
struct BaselineRecord {
    name: String,
    generation_id: String,
    findings_fingerprints: Vec<String>,
    findings: Vec<FindingRef>,
    created_at: String,
}

fn store() -> &'static Mutex<HashMap<String, BaselineRecord>> {
    static STORE: OnceLock<Mutex<HashMap<String, BaselineRecord>>> = OnceLock::new();
    STORE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Mirrors `baselineFingerprintSet(baseline)`.
pub fn baseline_fingerprint_set(findings: &[FindingRef]) -> HashSet<String> {
    findings.iter().map(|f| f.fingerprint.clone()).collect()
}

/// Mirrors `changedSlice({ findings, baseline, includeWorsened })`.
/// `includeWorsened` is accepted for contract parity but, matching the
/// legacy implementation, `worsenedCount` is always `0`.
pub fn changed_slice(findings: &[FindingRef], baseline: Option<&[FindingRef]>) -> ChangedSlice {
    let known = baseline_fingerprint_set(baseline.unwrap_or(&[]));
    let new_findings: Vec<FindingRef> = findings
        .iter()
        .filter(|f| !known.contains(&f.fingerprint))
        .cloned()
        .collect();
    let total = findings.len();
    ChangedSlice {
        total,
        new_count: new_findings.len(),
        worsened_count: 0,
        findings: new_findings,
        retained_total: total,
    }
}

#[derive(Debug, thiserror::Error)]
pub enum BaselineError {
    #[error("baseline name is required")]
    NameInvalid,
    #[error("generationId is required")]
    GenerationMissing,
}

/// Mirrors `captureNamedBaseline(name, { generationId, findings, createdAt })`.
pub fn capture_named_baseline(
    name: &str,
    generation_id: &str,
    findings: Vec<FindingRef>,
    created_at: Option<String>,
) -> Result<BaselineSummary, BaselineError> {
    let clean_name = name.trim().to_string();
    if clean_name.is_empty() {
        return Err(BaselineError::NameInvalid);
    }
    if generation_id.is_empty() {
        return Err(BaselineError::GenerationMissing);
    }
    let mut fingerprints: Vec<String> = baseline_fingerprint_set(&findings).into_iter().collect();
    fingerprints.sort();
    let created_at = created_at.unwrap_or_else(|| "1970-01-01T00:00:00.000Z".to_string());
    let record = BaselineRecord {
        name: clean_name.clone(),
        generation_id: generation_id.to_string(),
        findings_fingerprints: fingerprints.clone(),
        findings: findings.clone(),
        created_at: created_at.clone(),
    };
    let finding_count = findings.len();
    store().lock().unwrap().insert(clean_name.clone(), record);
    Ok(BaselineSummary {
        name: clean_name,
        generation_id: generation_id.to_string(),
        findings_fingerprints: fingerprints,
        finding_count,
        created_at,
    })
}

/// Mirrors `listNamedBaselines()`.
pub fn list_named_baselines() -> Vec<BaselineSummary> {
    let mut records: Vec<BaselineSummary> = store()
        .lock()
        .unwrap()
        .values()
        .map(|r| BaselineSummary {
            name: r.name.clone(),
            generation_id: r.generation_id.clone(),
            findings_fingerprints: r.findings_fingerprints.clone(),
            finding_count: r.findings.len(),
            created_at: r.created_at.clone(),
        })
        .collect();
    records.sort_by(|a, b| a.name.cmp(&b.name));
    records
}

/// Mirrors `getNamedBaseline(name)`.
pub fn get_named_baseline(name: &str) -> Option<BaselineSummary> {
    let clean = name.trim();
    store().lock().unwrap().get(clean).map(|r| BaselineSummary {
        name: r.name.clone(),
        generation_id: r.generation_id.clone(),
        findings_fingerprints: r.findings_fingerprints.clone(),
        finding_count: r.findings.len(),
        created_at: r.created_at.clone(),
    })
}

/// Mirrors `clearNamedBaselines()`.
pub fn clear_named_baselines() {
    store().lock().unwrap().clear();
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex as StdMutex;
    use std::sync::OnceLock as StdOnceLock;

    // Serialize tests since the baseline store is process-global (mirrors the
    // legacy module-level Map).
    fn test_lock() -> &'static StdMutex<()> {
        static LOCK: StdOnceLock<StdMutex<()>> = StdOnceLock::new();
        LOCK.get_or_init(|| StdMutex::new(()))
    }

    fn finding(fp: &str) -> FindingRef {
        FindingRef {
            fingerprint: fp.to_string(),
            rule_id: Some("r1".to_string()),
            path: None,
            name: None,
            specifier: None,
        }
    }

    #[test]
    fn changed_slice_reports_only_new_findings() {
        let baseline = vec![finding("a"), finding("b")];
        let findings = vec![finding("a"), finding("b"), finding("c")];
        let slice = changed_slice(&findings, Some(&baseline));
        assert_eq!(slice.total, 3);
        assert_eq!(slice.new_count, 1);
        assert_eq!(slice.retained_total, 3);
        assert_eq!(slice.findings.len(), 1);
        assert_eq!(slice.findings[0].fingerprint, "c");
    }

    #[test]
    fn capture_list_get_and_clear_named_baseline() {
        let _guard = test_lock().lock().unwrap();
        clear_named_baselines();
        let summary = capture_named_baseline(
            "release-1",
            "gen-1",
            vec![finding("b"), finding("a")],
            Some("2026-01-01T00:00:00.000Z".to_string()),
        )
        .unwrap();
        assert_eq!(summary.name, "release-1");
        assert_eq!(summary.findings_fingerprints, vec!["a".to_string(), "b".to_string()]);
        assert_eq!(summary.finding_count, 2);

        let listed = list_named_baselines();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].name, "release-1");

        let fetched = get_named_baseline("release-1").unwrap();
        assert_eq!(fetched.generation_id, "gen-1");

        clear_named_baselines();
        assert!(list_named_baselines().is_empty());
        assert!(get_named_baseline("release-1").is_none());
    }

    #[test]
    fn empty_name_is_rejected() {
        let err = capture_named_baseline("   ", "gen-1", vec![], None).unwrap_err();
        assert!(matches!(err, BaselineError::NameInvalid));
    }

    #[test]
    fn missing_generation_id_is_rejected() {
        let err = capture_named_baseline("name", "", vec![], None).unwrap_err();
        assert!(matches!(err, BaselineError::GenerationMissing));
    }
}
