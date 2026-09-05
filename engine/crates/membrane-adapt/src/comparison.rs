//! ADP-076: bounded interpretation of externally evaluated candidates.
//!
//! This module cannot run a model, alter a target, admit a preference or grant
//! authority. Receipt references identify evaluator evidence supplied by the
//! host; the caller must resolve/authorize those references before activation.
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ComparisonLimitsV1 {
    pub candidates: usize,
    pub cases: usize,
    pub evaluator_calls: u64,
    pub proposal_iterations: u64,
    pub cost_microunits: u64,
    pub elapsed_ms: u64,
    pub concurrency: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ComparisonUsageV1 {
    pub evaluator_calls: u64,
    pub proposal_iterations: u64,
    pub cost_microunits: u64,
    pub elapsed_ms: u64,
    pub concurrency: u32,
}

/// Domain projection of an evaluator result, not a raw execution observation.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CandidateCaseV1 {
    pub candidate_sha256: String,
    pub case_id: String,
    pub case_sha256: String,
    pub stratum: String,
    pub receipt_id: String,
    pub correct: bool,
    pub adherent: bool,
    pub recurred: bool,
    pub false_block: bool,
    pub authority_violation: bool,
    pub latency_ms: u64,
    pub cost_microunits: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CandidateComparisonV1 {
    pub schema_version: u32,
    pub comparison_id: String,
    pub target: String,
    pub target_version: u64,
    pub scope: String,
    pub allowed_change_sha256: String,
    pub baseline_sha256: String,
    pub candidates: Vec<String>,
    pub development_dataset_sha256: String,
    pub test_dataset_sha256: String,
    pub evaluator_sha256: String,
    pub host_configuration_sha256: String,
    pub limits: ComparisonLimitsV1,
    pub usage: ComparisonUsageV1,
    pub cancelled: bool,
    pub development: Vec<CandidateCaseV1>,
    /// Only baseline and the development-selected candidate may appear here.
    pub frozen_test: Vec<CandidateCaseV1>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ComparisonDisposition {
    CandidateSelected,
    NoImprovement,
    InsufficientEvidence,
    Cancelled,
    BudgetExhausted,
    Regression,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComparisonDecisionV1 {
    pub contract: String,
    pub comparison_id: String,
    pub request_sha256: String,
    pub target: String,
    pub target_version: u64,
    pub scope: String,
    pub baseline_sha256: String,
    pub selected_sha256: String,
    pub disposition: ComparisonDisposition,
    pub reason: String,
    pub evidence_basis: String,
    pub requires_independent_admission: bool,
    pub requires_target_revalidation: bool,
    pub activation_authorized: bool,
    pub decision_sha256: String,
}

pub(crate) fn digest(value: &impl Serialize) -> String {
    crate::canonical::sha256_canonical(
        &serde_json::to_value(value).expect("domain types serialize"),
    )
}
pub(crate) fn valid_digest(value: &str) -> bool {
    let value = value.strip_prefix("sha256:").unwrap_or(value);
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
}
pub(crate) fn bounded_id(value: &str) -> bool {
    !value.trim().is_empty() && value.len() <= 512 && !value.chars().any(char::is_control)
}

type Cases<'a> = BTreeMap<&'a str, &'a CandidateCaseV1>;
fn cases<'a>(rows: &'a [CandidateCaseV1], candidate: &str) -> Cases<'a> {
    rows.iter()
        .filter(|r| r.candidate_sha256 == candidate)
        .map(|r| (r.case_id.as_str(), r))
        .collect()
}
fn compatible(base: &Cases<'_>, candidate: &Cases<'_>) -> bool {
    !base.is_empty()
        && base.len() == candidate.len()
        && base.iter().all(|(id, b)| {
            candidate
                .get(id)
                .is_some_and(|c| b.case_sha256 == c.case_sha256 && b.stratum == c.stratum)
        })
}
fn quality(base: &Cases<'_>, candidate: &Cases<'_>) -> Option<usize> {
    if !compatible(base, candidate) {
        return None;
    }
    let mut improvements = 0;
    for (id, b) in base {
        let c = candidate[id];
        // A faster/cheaper candidate cannot buy a correctness, authority,
        // false-block or successful-task regression with a better average.
        if c.authority_violation
            || (!b.false_block && c.false_block)
            || (b.correct && !c.correct)
            || (b.adherent && !c.adherent)
            || (!b.recurred && c.recurred)
        {
            return None;
        }
        improvements += usize::from(
            (!b.correct && c.correct)
                || (!b.adherent && c.adherent)
                || (b.recurred && !c.recurred)
                || (b.false_block && !c.false_block),
        );
    }
    Some(improvements)
}
fn has_control_strata(rows: &Cases<'_>) -> bool {
    ["successful", "hard_negative", "nonapplicable"]
        .iter()
        .all(|s| rows.values().any(|r| r.stratum == *s))
}

pub fn compare(request: &CandidateComparisonV1) -> Result<ComparisonDecisionV1, String> {
    if request.schema_version != 1
        || !bounded_id(&request.comparison_id)
        || !bounded_id(&request.target)
        || !bounded_id(&request.scope)
    {
        return Err("invalid comparison identity/schema".into());
    }
    for d in [
        &request.allowed_change_sha256,
        &request.baseline_sha256,
        &request.development_dataset_sha256,
        &request.test_dataset_sha256,
        &request.evaluator_sha256,
        &request.host_configuration_sha256,
    ] {
        if !valid_digest(d) {
            return Err("comparison requires exact SHA-256 bindings".into());
        }
    }
    let limits = &request.limits;
    if limits.candidates == 0
        || limits.candidates > 16
        || limits.cases == 0
        || limits.cases > 2048
        || limits.concurrency == 0
        || limits.concurrency > 16
        || limits.evaluator_calls == 0
        || limits.proposal_iterations == 0
        || limits.elapsed_ms == 0
    {
        return Err("invalid comparison bounds".into());
    }
    let mut ids = BTreeSet::new();
    ids.insert(request.baseline_sha256.as_str());
    for c in &request.candidates {
        if !valid_digest(c) || !ids.insert(c.as_str()) {
            return Err("invalid/duplicate candidate".into());
        }
    }
    if request.candidates.len() > 16
        || request.development.len() > 32768
        || request.frozen_test.len() > 4096
    {
        return Err("comparison exceeds hard input bounds".into());
    }
    let mut receipts = BTreeSet::new();
    let mut dev_cases = BTreeMap::new();
    let mut test_cases = BTreeMap::new();
    for (rows, cases_by_id) in [
        (&request.development, &mut dev_cases),
        (&request.frozen_test, &mut test_cases),
    ] {
        let mut pairs = BTreeSet::new();
        for row in rows {
            if !ids.contains(row.candidate_sha256.as_str())
                || !bounded_id(&row.case_id)
                || !valid_digest(&row.case_sha256)
                || !bounded_id(&row.stratum)
                || !bounded_id(&row.receipt_id)
                || !receipts.insert(&row.receipt_id)
                || !pairs.insert((&row.candidate_sha256, &row.case_id))
            {
                return Err("invalid, duplicated or unbound evaluator row".into());
            }
            if let Some(prior) = cases_by_id.insert(row.case_id.clone(), row.case_sha256.clone()) {
                if prior != row.case_sha256 {
                    return Err("case digest changed within comparison".into());
                }
            }
        }
    }
    if request.development_dataset_sha256 == request.test_dataset_sha256
        || test_cases
            .iter()
            .any(|(id, hash)| dev_cases.contains_key(id) || dev_cases.values().any(|h| h == hash))
    {
        return Err("development/frozen-test leakage".into());
    }
    let decision = |disposition, selected: &str, reason: &str| {
        let mut result = ComparisonDecisionV1 {
            contract: "adapt.candidate-comparison-decision.v1".into(),
            comparison_id: request.comparison_id.clone(),
            request_sha256: digest(request),
            target: request.target.clone(),
            target_version: request.target_version,
            scope: request.scope.clone(),
            baseline_sha256: request.baseline_sha256.clone(),
            selected_sha256: selected.into(),
            disposition,
            reason: reason.into(),
            evidence_basis: "host_supplied_evaluator_receipts".into(),
            requires_independent_admission: true,
            requires_target_revalidation: true,
            activation_authorized: false,
            decision_sha256: String::new(),
        };
        result.decision_sha256 = digest(&result);
        result
    };
    let baseline = request.baseline_sha256.as_str();
    if request.cancelled {
        return Ok(decision(
            ComparisonDisposition::Cancelled,
            baseline,
            "cancelled",
        ));
    }
    let usage = &request.usage;
    if request.candidates.len() > limits.candidates
        || dev_cases.len() + test_cases.len() > limits.cases
        || usage.evaluator_calls > limits.evaluator_calls
        || usage.proposal_iterations > limits.proposal_iterations
        || usage.cost_microunits > limits.cost_microunits
        || usage.elapsed_ms > limits.elapsed_ms
        || usage.concurrency > limits.concurrency
    {
        return Ok(decision(
            ComparisonDisposition::BudgetExhausted,
            baseline,
            "declared_budget_exhausted",
        ));
    }
    let base = cases(&request.development, baseline);
    if !has_control_strata(&base) {
        return Ok(decision(
            ComparisonDisposition::InsufficientEvidence,
            baseline,
            "development_controls_missing",
        ));
    }
    let mut ranked: Vec<_> = request
        .candidates
        .iter()
        .filter_map(|c| {
            quality(&base, &cases(&request.development, c))
                .filter(|n| *n > 0)
                .map(|n| (n, c))
        })
        .collect();
    ranked.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(b.1)));
    let Some((_, selected)) = ranked.first() else {
        return Ok(decision(
            ComparisonDisposition::NoImprovement,
            baseline,
            "no_safe_development_improvement",
        ));
    };
    if request
        .frozen_test
        .iter()
        .any(|r| r.candidate_sha256 != baseline && &r.candidate_sha256 != *selected)
    {
        return Err("frozen test may evaluate only the development winner and baseline".into());
    }
    let test_base = cases(&request.frozen_test, baseline);
    let test_candidate = cases(&request.frozen_test, selected);
    if !has_control_strata(&test_base) || !compatible(&test_base, &test_candidate) {
        return Ok(decision(
            ComparisonDisposition::InsufficientEvidence,
            baseline,
            "frozen_test_incomplete",
        ));
    }
    match quality(&test_base, &test_candidate) {
        Some(n) if n > 0 => Ok(decision(
            ComparisonDisposition::CandidateSelected,
            selected,
            "independent_admission_still_required",
        )),
        Some(_) => Ok(decision(
            ComparisonDisposition::NoImprovement,
            baseline,
            "no_frozen_test_improvement",
        )),
        None => Ok(decision(
            ComparisonDisposition::Regression,
            baseline,
            "frozen_test_regression",
        )),
    }
}
