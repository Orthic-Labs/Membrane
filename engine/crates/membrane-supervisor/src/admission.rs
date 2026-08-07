//! Admission policy for component leases. The supervisor and its resident
//! compare their component leases before joining the live service so a stale
//! build, a divergent data root, or an expired authority cannot silently
//! piggy-back onto a fresh installation.
//!
//! The decision is deterministic and pure given a monotonic clock reading: a
//! candidate lease is admitted when it shares installation identity and
//! release identity with the active lease, points at the same canonical data
//! root, and is not yet expired. Any divergence produces a typed rejection
//! rather than a silent admit, so the supervisor never lets an old build share
//! a live service with a new one.
//!
//! MBR-202: implement exact-build and data-root admission leases.

use membrane_protocol::ComponentLeaseV1;
use serde::{Deserialize, Serialize};
use std::time::SystemTime;

/// Typed admission decision returned by [`evaluate`].
///
/// `Admit` carries the candidate lease so the caller can hand it downstream
/// without re-reading its fields. Each `Reject*` variant carries the two
/// values the supervisor compared — the active lease's `observed_*` and the
/// candidate's `candidate_*` — so the rejection can be surfaced verbatim in
/// logs, status files, and structured rejection responses.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum AdmissionDecision {
    /// The candidate lease is compatible with the active lease and may join
    /// the live service. The `lease` field carries the candidate so the
    /// caller can forward it without re-reading the source.
    Admit { lease: ComponentLeaseV1 },
    /// Same installation, but the candidate was built from a different
    /// verified source tree than the active lease. The candidate is an old
    /// (or future) build pretending to be the active one; an old build must
    /// never share the live service with a freshly issued lease.
    RejectOldBuild {
        observed_release_generation: String,
        candidate_release_generation: String,
    },
    /// Same installation and build, but the candidate points at a different
    /// canonical data root than the active lease. Two builds of the same
    /// source tree must never share a writable data root.
    RejectWrongDataRoot {
        observed_data_root_digest: String,
        candidate_data_root_digest: String,
    },
    /// Same installation, build, and data root, but the candidate's expiry
    /// is in the past. The candidate is a stale lease; a fresh active lease
    /// must replace it before any IPC is admitted.
    RejectStaleLease {
        observed_expires_at: String,
        candidate_expires_at: String,
    },
}

impl AdmissionDecision {
    /// Stable short label suitable for log routing. Always lowercase, never
    /// user-facing prose.
    pub fn label(&self) -> &'static str {
        match self {
            AdmissionDecision::Admit { .. } => "admit",
            AdmissionDecision::RejectOldBuild { .. } => "reject-old-build",
            AdmissionDecision::RejectWrongDataRoot { .. } => "reject-wrong-data-root",
            AdmissionDecision::RejectStaleLease { .. } => "reject-stale-lease",
        }
    }

    /// `true` when the candidate lease is admissible against the active one.
    pub fn is_admitted(&self) -> bool {
        matches!(self, AdmissionDecision::Admit { .. })
    }
}

/// Compare two component leases for live-service compatibility.
///
/// Decision precedence is deterministic and mirrors the order in which an
/// installation is bound:
///   1. A different `installation_id` always admits: distinct installations
///      on the same machine are independent live services and must coexist.
///   2. The same installation but a different `release_generation` is a
///      rejected old build: a stale verified source tree must not share the
///      live service with a freshly issued one.
///   3. The same installation and build but a different `data_root_digest`
///      is a rejected wrong data root: two builds of the same source tree
///      must never share a writable data root.
///   4. The same installation, build, and data root with an expired
///      `expires_at` is a rejected stale lease: the active lease has
///      advanced past the candidate's validity window.
///   5. Anything else (all four identity fields match, signature out-of-band,
///      and expiry still in the future) is admitted.
pub fn evaluate(candidate: &ComponentLeaseV1, active: &ComponentLeaseV1) -> AdmissionDecision {
    if candidate.installation_id != active.installation_id {
        return AdmissionDecision::Admit {
            lease: candidate.clone(),
        };
    }
    if candidate.release_generation != active.release_generation {
        return AdmissionDecision::RejectOldBuild {
            observed_release_generation: active.release_generation.clone(),
            candidate_release_generation: candidate.release_generation.clone(),
        };
    }
    if candidate.data_root_digest != active.data_root_digest {
        return AdmissionDecision::RejectWrongDataRoot {
            observed_data_root_digest: active.data_root_digest.clone(),
            candidate_data_root_digest: candidate.data_root_digest.clone(),
        };
    }
    if lease_is_expired(&candidate.expires_at) {
        return AdmissionDecision::RejectStaleLease {
            observed_expires_at: active.expires_at.clone(),
            candidate_expires_at: candidate.expires_at.clone(),
        };
    }
    AdmissionDecision::Admit {
        lease: candidate.clone(),
    }
}

/// `true` when `expires_at` (RFC 3339 UTC `Z`-suffix, whole seconds) is at or
/// before the current monotonic clock. Unparseable timestamps are treated as
/// expired so a malformed lease cannot accidentally bypass the gate.
fn lease_is_expired(expires_at: &str) -> bool {
    match parse_rfc3339_utc(expires_at) {
        Some(expiry) => expiry <= now_unix_secs(),
        None => true,
    }
}

/// Parse the subset of RFC 3339 we accept: `YYYY-MM-DDTHH:MM:SSZ`. The lease
/// shape forbids offsets and fractional seconds because every emitter is
/// internal and produces whole-second timestamps in UTC. Returns `None` for
/// anything else.
fn parse_rfc3339_utc(value: &str) -> Option<i64> {
    let bytes = value.as_bytes();
    if bytes.len() != 20 || bytes[10] != b'T' || bytes[bytes.len() - 1] != b'Z' {
        return None;
    }
    let year = digits(&bytes[0..4])?;
    let month = digits(&bytes[5..7])?;
    let day = digits(&bytes[8..10])?;
    let hour = digits(&bytes[11..13])?;
    let minute = digits(&bytes[14..16])?;
    let second = digits(&bytes[17..19])?;
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    Some(utc_seconds_since_epoch(
        year, month, day, hour, minute, second,
    ))
}

fn digits(slice: &[u8]) -> Option<i64> {
    let mut total: i64 = 0;
    for byte in slice {
        if !byte.is_ascii_digit() {
            return None;
        }
        total = total.checked_mul(10)?.checked_add(i64::from(byte - b'0'))?;
    }
    Some(total)
}

/// Seconds since the Unix epoch for `YYYY-MM-DD HH:MM:SS` in UTC. Uses the
/// Howard Hinnant days_from_civil algorithm (CC0). Out-of-range dates return
/// `i64::MIN` so callers that compare against `now_unix_secs` see a clearly
/// expired lease rather than a panic.
fn utc_seconds_since_epoch(
    year: i64,
    month: i64,
    day: i64,
    hour: i64,
    minute: i64,
    second: i64,
) -> i64 {
    let days = days_from_civil(year, month, day);
    let total_seconds = days
        .checked_mul(86_400)
        .and_then(|s| s.checked_add(hour * 3600 + minute * 60 + second));
    total_seconds.unwrap_or(i64::MIN)
}

/// Howard Hinnant's days_from_civil (CC0). Returns the signed number of days
/// from the Unix epoch (1970-01-01) to the supplied civil date in UTC.
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = if year >= 0 { year } else { year - 399 } / 400;
    let yoe = year - era * 400;
    let m = if month > 2 { month - 3 } else { month + 9 };
    let doy = (153 * m + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

fn now_unix_secs() -> i64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_lease() -> ComponentLeaseV1 {
        ComponentLeaseV1 {
            installation_id: "00000000-0000-4000-8000-000000000001".to_string(),
            release_generation: format!("sha256:{}", "1".repeat(64)),
            data_root_digest: format!("sha256:{}", "2".repeat(64)),
            issued_at: "2026-08-07T00:00:00Z".to_string(),
            expires_at: "2099-01-01T00:00:00Z".to_string(),
            signature: "lease-sig-test".to_string(),
        }
    }

    #[test]
    fn matching_leases_admit() {
        let active = sample_lease();
        let candidate = active.clone();
        match evaluate(&candidate, &active) {
            AdmissionDecision::Admit { lease } => assert_eq!(lease, candidate),
            other => panic!("expected Admit, got {other:?}"),
        }
    }

    #[test]
    fn different_installation_id_admits() {
        let active = sample_lease();
        let mut candidate = active.clone();
        candidate.installation_id = "00000000-0000-4000-8000-000000000002".to_string();
        assert!(matches!(
            evaluate(&candidate, &active),
            AdmissionDecision::Admit { .. }
        ));
    }

    #[test]
    fn different_release_generation_rejects_old_build() {
        let active = sample_lease();
        let mut candidate = active.clone();
        candidate.release_generation = format!("sha256:{}", "a".repeat(64));
        match evaluate(&candidate, &active) {
            AdmissionDecision::RejectOldBuild {
                observed_release_generation,
                candidate_release_generation,
            } => {
                assert_eq!(observed_release_generation, active.release_generation);
                assert_eq!(candidate_release_generation, candidate.release_generation);
            }
            other => panic!("expected RejectOldBuild, got {other:?}"),
        }
    }

    #[test]
    fn different_data_root_rejects_wrong_data_root() {
        let active = sample_lease();
        let mut candidate = active.clone();
        candidate.data_root_digest = format!("sha256:{}", "b".repeat(64));
        match evaluate(&candidate, &active) {
            AdmissionDecision::RejectWrongDataRoot {
                observed_data_root_digest,
                candidate_data_root_digest,
            } => {
                assert_eq!(observed_data_root_digest, active.data_root_digest);
                assert_eq!(candidate_data_root_digest, candidate.data_root_digest);
            }
            other => panic!("expected RejectWrongDataRoot, got {other:?}"),
        }
    }

    #[test]
    fn expired_lease_rejects_stale_lease() {
        let active = sample_lease();
        let mut candidate = active.clone();
        candidate.expires_at = "1999-01-01T00:00:00Z".to_string();
        match evaluate(&candidate, &active) {
            AdmissionDecision::RejectStaleLease {
                observed_expires_at,
                candidate_expires_at,
            } => {
                assert_eq!(observed_expires_at, active.expires_at);
                assert_eq!(candidate_expires_at, candidate.expires_at);
            }
            other => panic!("expected RejectStaleLease, got {other:?}"),
        }
    }

    #[test]
    fn unparseable_expiry_is_treated_as_expired() {
        let active = sample_lease();
        let mut candidate = active.clone();
        candidate.expires_at = "not-a-timestamp".to_string();
        assert!(matches!(
            evaluate(&candidate, &active),
            AdmissionDecision::RejectStaleLease { .. }
        ));
    }

    #[test]
    fn parse_rfc3339_accepts_canonical_z_form() {
        let secs = parse_rfc3339_utc("2026-08-07T12:00:00Z").unwrap();
        assert!(secs > 1_700_000_000);
    }

    #[test]
    fn parse_rfc3339_rejects_offset_and_fractional() {
        assert!(parse_rfc3339_utc("2026-08-07T12:00:00+00:00").is_none());
        assert!(parse_rfc3339_utc("2026-08-07T12:00:00.000Z").is_none());
        assert!(parse_rfc3339_utc("not-a-timestamp").is_none());
    }

    #[test]
    fn admission_decision_labels_are_stable() {
        let active = sample_lease();
        assert_eq!(evaluate(&active.clone(), &active).label(), "admit");
        let mut other_install = active.clone();
        other_install.installation_id = "00000000-0000-4000-8000-000000000099".to_string();
        assert_eq!(evaluate(&other_install, &active).label(), "admit");
        let mut other_release = active.clone();
        other_release.release_generation = format!("sha256:{}", "c".repeat(64));
        assert_eq!(
            evaluate(&other_release, &active).label(),
            "reject-old-build"
        );
        let mut other_root = active.clone();
        other_root.data_root_digest = format!("sha256:{}", "d".repeat(64));
        assert_eq!(
            evaluate(&other_root, &active).label(),
            "reject-wrong-data-root"
        );
        let mut other_expiry = active.clone();
        other_expiry.expires_at = "1999-01-01T00:00:00Z".to_string();
        assert_eq!(
            evaluate(&other_expiry, &active).label(),
            "reject-stale-lease"
        );
    }
}
