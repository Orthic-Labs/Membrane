//! Read-only fleet/replication projection.
//! Inputs are authoritative observations; missing fields stay unknown.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FleetInstallation {
    pub installation_id: String,
    pub state: String,
    pub reason: String,
    pub source: String,
    pub evidence: String,
    pub resolver: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReplicationObservation {
    pub observer_id: String,
    pub origin_id: String,
    pub state: String,
    pub reason: String,
    pub source: String,
    pub evidence: String,
    pub resolver: String,
    pub applied_seq: Option<u64>,
    pub available_seq: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FleetView {
    pub schema_version: u32,
    pub state: String,
    pub reason: String,
    pub source: String,
    pub evidence: String,
    pub resolver: String,
    pub installations: Vec<FleetInstallation>,
    pub replication: Vec<ReplicationObservation>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FleetProjectionError {
    TooManyRows,
    MissingEvidence,
}

const MAX_FLEET_ROWS: usize = 128;

/// Materialize a view without synthesizing health, liveness, peers, or rows.
/// Callers must provide authoritative installations and replication observations.
pub fn project_fleet(
    mut installations: Vec<FleetInstallation>,
    mut replication: Vec<ReplicationObservation>,
    state: impl Into<String>,
    reason: impl Into<String>,
    source: impl Into<String>,
    evidence: impl Into<String>,
    resolver: impl Into<String>,
) -> Result<FleetView, FleetProjectionError> {
    if installations.len() + replication.len() > MAX_FLEET_ROWS {
        return Err(FleetProjectionError::TooManyRows);
    }
    let present = |value: &str| !value.trim().is_empty();
    let values = [
        state.into(),
        reason.into(),
        source.into(),
        evidence.into(),
        resolver.into(),
    ];
    let installation_ok = |row: &FleetInstallation| {
        [
            &row.installation_id,
            &row.state,
            &row.reason,
            &row.source,
            &row.evidence,
            &row.resolver,
        ]
        .iter()
        .all(|value| present(value))
    };
    let replication_ok = |row: &ReplicationObservation| {
        [
            &row.observer_id,
            &row.origin_id,
            &row.state,
            &row.reason,
            &row.source,
            &row.evidence,
            &row.resolver,
        ]
        .iter()
        .all(|value| present(value))
    };
    if !values.iter().all(|value| present(value))
        || !installations.iter().all(installation_ok)
        || !replication.iter().all(replication_ok)
    {
        return Err(FleetProjectionError::MissingEvidence);
    }
    installations.sort_by(|a, b| a.installation_id.cmp(&b.installation_id));
    replication.sort_by(|a, b| (&a.observer_id, &a.origin_id).cmp(&(&b.observer_id, &b.origin_id)));
    let [state, reason, source, evidence, resolver] = values;
    Ok(FleetView {
        schema_version: 1,
        state,
        reason,
        source,
        evidence,
        resolver,
        installations,
        replication,
    })
}
