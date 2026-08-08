//! Pure maintenance planning for low-priority Membrane housekeeping (MBR-508).
//!
//! This module does not own a store, spawn work, or certify authority. A host supplies a
//! verified authority receipt, executes returned operations outside foreground handling, then
//! persists the returned content-free receipt through its existing receipt plane.

use serde::{Deserialize, Serialize};

pub const MAINTENANCE_RECEIPT_SCHEMA_VERSION: u32 = 1;
pub const MAX_MAINTENANCE_BUDGET_UNITS: u32 = 10_000;
pub const MAX_MAINTENANCE_WINDOW_MS: u64 = 900_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MaintenanceKind {
    Consolidation,
    ContradictionDetection,
    IndexMaintenance,
    ProposalCreation,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AuthorityReceipt {
    pub receipt_id: String,
    pub issuer_id: String,
    pub subject_id: String,
    pub scope: String,
    pub allowed_kinds: Vec<MaintenanceKind>,
}

pub trait MaintenanceAuthorityVerifier {
    fn verify(&self, receipt: &AuthorityReceipt) -> bool;
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MaintenanceRequest {
    pub request_id: String,
    pub scheduler_id: String,
    pub kind: MaintenanceKind,
    pub authority: AuthorityReceipt,
    pub budget_units: u32,
    pub deadline_unix_ms: u64,
    pub now_unix_ms: u64,
    pub cancelled: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MaintenanceDisposition {
    Scheduled,
    Rejected,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MaintenanceReason {
    AuthorityMissing,
    AuthoritySelfCertified,
    AuthorityUnverified,
    AuthorityScopeDenied,
    AuthorityCapabilityDenied,
    BudgetExhausted,
    BudgetExceeded,
    DeadlineExpired,
    DeadlineTooLong,
    Cancelled,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MaintenanceOperation {
    pub request_id: String,
    pub kind: MaintenanceKind,
    pub budget_units: u32,
    pub deadline_unix_ms: u64,
    pub scheduler_id: String,
    pub authority_receipt_id: String,
}

/// Content-free planning receipt. It never carries document text, embeddings, paths, or results.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MaintenanceReceipt {
    pub schema_version: u32,
    pub request_id: String,
    pub scheduler_id: String,
    pub kind: MaintenanceKind,
    pub disposition: MaintenanceDisposition,
    pub reason: Option<MaintenanceReason>,
    pub authority_receipt_id: Option<String>,
    pub budget_units: u32,
    pub deadline_unix_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MaintenancePlan {
    pub receipt: MaintenanceReceipt,
    pub operation: Option<MaintenanceOperation>,
}

/// Evaluate a bounded request. Callers execute only `operation`, outside foreground handling.
pub fn plan<V: MaintenanceAuthorityVerifier>(request: &MaintenanceRequest, verifier: &V) -> MaintenancePlan {
    let rejection = validate(request, verifier);
    let scheduled = rejection.is_none();
    let receipt = MaintenanceReceipt {
        schema_version: MAINTENANCE_RECEIPT_SCHEMA_VERSION,
        request_id: request.request_id.clone(),
        scheduler_id: request.scheduler_id.clone(),
        kind: request.kind,
        disposition: if scheduled { MaintenanceDisposition::Scheduled } else { MaintenanceDisposition::Rejected },
        reason: rejection,
        authority_receipt_id: nonempty(&request.authority.receipt_id).then(|| request.authority.receipt_id.clone()),
        budget_units: request.budget_units,
        deadline_unix_ms: request.deadline_unix_ms,
    };
    let operation = scheduled.then(|| MaintenanceOperation {
        request_id: request.request_id.clone(),
        kind: request.kind,
        budget_units: request.budget_units,
        deadline_unix_ms: request.deadline_unix_ms,
        scheduler_id: request.scheduler_id.clone(),
        authority_receipt_id: request.authority.receipt_id.clone(),
    });
    MaintenancePlan { receipt, operation }
}

fn validate<V: MaintenanceAuthorityVerifier>(request: &MaintenanceRequest, verifier: &V) -> Option<MaintenanceReason> {
    if request.cancelled { return Some(MaintenanceReason::Cancelled); }
    if !safe_id(&request.request_id) || !safe_id(&request.scheduler_id) { return Some(MaintenanceReason::AuthorityMissing); }
    if request.budget_units == 0 { return Some(MaintenanceReason::BudgetExhausted); }
    if request.budget_units > MAX_MAINTENANCE_BUDGET_UNITS { return Some(MaintenanceReason::BudgetExceeded); }
    if request.deadline_unix_ms <= request.now_unix_ms { return Some(MaintenanceReason::DeadlineExpired); }
    if request.deadline_unix_ms.saturating_sub(request.now_unix_ms) > MAX_MAINTENANCE_WINDOW_MS { return Some(MaintenanceReason::DeadlineTooLong); }
    if !nonempty(&request.authority.receipt_id) || !nonempty(&request.authority.issuer_id) {
        return Some(MaintenanceReason::AuthorityMissing);
    }
    if request.authority.issuer_id == request.scheduler_id || request.authority.subject_id != request.scheduler_id {
        return Some(MaintenanceReason::AuthoritySelfCertified);
    }
    if !verifier.verify(&request.authority) { return Some(MaintenanceReason::AuthorityUnverified); }
    if request.authority.scope != "maintenance" { return Some(MaintenanceReason::AuthorityScopeDenied); }
    if !request.authority.allowed_kinds.contains(&request.kind) { return Some(MaintenanceReason::AuthorityCapabilityDenied); }
    None
}

fn nonempty(value: &str) -> bool { !value.trim().is_empty() }
fn safe_id(value: &str) -> bool { !value.is_empty() && value.len() <= 160 && value.bytes().all(|byte| byte.is_ascii_alphanumeric() || b"._:-".contains(&byte)) }
