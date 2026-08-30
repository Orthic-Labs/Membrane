//! MBR-912: machine-checkable, fail-closed release-channel compatibility
//! policy. This is the enforcement point, not just documentation of one --
//! `release/channels/README.md` and `docs/product/compatibility/release-channels.md`
//! describe the shape and intent in prose; this module is the code a
//! caller (a future Hub/CLI admission path, or this crate's own tests)
//! actually runs to get an admit/refuse decision.
//!
//! The policy refuses by default on exactly the three cases MBR-912 names:
//!
//!   1. an unknown channel (any `channel` string other than `stable`,
//!      `beta`, `nightly`),
//!   2. an unsupported release-channel schema version (anything other than
//!      [`RELEASE_CHANNEL_SCHEMA_VERSION`]), and
//!   3. a downgrade (a candidate `release` that is not a strictly greater
//!      `major.minor.patch` than the locally installed version).
//!
//! An explicitly `unknown` or `incompatible` `schemaCompatibility` value is
//! refused too, for the same fail-closed reason: `schemaCompatibility` must
//! be *proven* `compatible` or `migration_required` before admission, never
//! assumed compatible by default.
//!
//! [`evaluate_compatibility`] is the one enforcement path. Both an untrusted
//! wire value (four raw strings/one integer, exactly as they would arrive
//! over IPC or a `--channel` CLI flag, before any typed validation exists)
//! and an already-typed, schema-valid [`ReleaseChannelV1`] go through it --
//! [`evaluate_release_channel`] converts the latter into the same raw shape
//! via [`RawReleaseDescriptor::from`] rather than special-casing "trusted"
//! input, so there is exactly one place this policy can diverge from itself.
//!
//! This module never contradicts membrane-updater's (MBR-911, see
//! `engine/crates/membrane-updater/src/lib.rs`) downgrade rejection: both
//! use the identical "strictly greater `major.minor.patch` triple, treat an
//! unparseable version as fail-closed" rule. The two copies are independent
//! because membrane-protocol is a lower-layer contract crate that must not
//! depend on membrane-updater (no crate in `engine/crates/` currently
//! depends on membrane-updater, and membrane-updater depends on nothing but
//! `serde`); any future drift between the two rules would be a real defect,
//! flagged here and at membrane-updater's `is_upgrade` for exactly that
//! reason.

use crate::release_channel::{
    ReleaseChannel, ReleaseChannelV1, SchemaCompatibility, RELEASE_CHANNEL_SCHEMA_VERSION,
};

/// One reason `evaluate_compatibility` refused a candidate release. Every
/// variant maps to a stable `code()` string so a caller (CLI, log line,
/// receipt) can report *why* without re-deriving prose from the enum shape.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CompatibilityViolation {
    UnknownChannel(String),
    UnsupportedSchemaVersion(u32),
    UnknownSchemaCompatibility(String),
    IncompatibleSchema,
    Downgrade { current: String, candidate: String },
    UnparseableVersion(String),
}

impl CompatibilityViolation {
    pub const fn code(&self) -> &'static str {
        match self {
            Self::UnknownChannel(_) => "release_channel_unknown",
            Self::UnsupportedSchemaVersion(_) => "release_channel_schema_version_unsupported",
            Self::UnknownSchemaCompatibility(_) => "release_channel_schema_compatibility_unknown",
            Self::IncompatibleSchema => "release_channel_schema_incompatible",
            Self::Downgrade { .. } => "release_channel_downgrade_rejected",
            Self::UnparseableVersion(_) => "release_channel_version_unparseable",
        }
    }
}

/// A release-channel descriptor exactly as it would arrive off the wire:
/// four untyped strings and one integer, none of them validated yet. This
/// is deliberately *not* [`ReleaseChannelV1`] -- building that type already
/// requires `channel` and `schemaCompatibility` to be one of their known
/// enum strings, which would make "unknown channel" and "unknown schema
/// compatibility" unreachable inputs instead of the runtime-checked
/// fail-closed decisions MBR-912 requires.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawReleaseDescriptor {
    pub schema_version: u32,
    pub channel: String,
    pub release: String,
    pub schema_compatibility: String,
}

impl From<&ReleaseChannelV1> for RawReleaseDescriptor {
    fn from(value: &ReleaseChannelV1) -> Self {
        Self {
            schema_version: value.schema_version,
            channel: value.channel.as_str().to_string(),
            release: value.release.clone(),
            schema_compatibility: value.schema_compatibility.as_str().to_string(),
        }
    }
}

/// A candidate release the policy admitted. `migration_required` is carried
/// forward rather than dropped: an admitted release can still require a
/// migration step before activation, and a caller that ignores this flag
/// and activates directly would violate the compatibility policy just as
/// much as one that ignored a `Downgrade` violation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdmittedRelease {
    pub channel: ReleaseChannel,
    pub release: String,
    pub migration_required: bool,
}

/// The one fail-closed enforcement path. Every check runs and every
/// violation is collected (not short-circuited on the first failure), so a
/// caller sees the full refusal reason set in one pass -- matching
/// membrane-updater's `verify()` style of returning every failure it found
/// rather than only the first. Returns `Ok` only when the channel is known,
/// the schema version is current, schema compatibility is explicitly
/// `compatible` or `migration_required`, and `release` is a strictly
/// greater version than `current_version`.
pub fn evaluate_compatibility(
    current_version: &str,
    candidate: &RawReleaseDescriptor,
) -> Result<AdmittedRelease, Vec<CompatibilityViolation>> {
    let mut violations = Vec::new();

    let channel = match parse_channel(&candidate.channel) {
        Ok(channel) => Some(channel),
        Err(violation) => {
            violations.push(violation);
            None
        }
    };

    if candidate.schema_version != RELEASE_CHANNEL_SCHEMA_VERSION {
        violations.push(CompatibilityViolation::UnsupportedSchemaVersion(
            candidate.schema_version,
        ));
    }

    let compatibility = parse_schema_compatibility(&candidate.schema_compatibility);
    match &compatibility {
        Ok(SchemaCompatibility::Incompatible) => {
            violations.push(CompatibilityViolation::IncompatibleSchema)
        }
        Ok(SchemaCompatibility::Unknown) => {
            violations.push(CompatibilityViolation::UnknownSchemaCompatibility(
                candidate.schema_compatibility.clone(),
            ))
        }
        Ok(SchemaCompatibility::Compatible) | Ok(SchemaCompatibility::MigrationRequired) => {}
        Err(violation) => violations.push(violation.clone()),
    }
    let migration_required = compatibility == Ok(SchemaCompatibility::MigrationRequired);

    match is_strict_upgrade(current_version, &candidate.release) {
        Ok(true) => {}
        Ok(false) => violations.push(CompatibilityViolation::Downgrade {
            current: current_version.to_string(),
            candidate: candidate.release.clone(),
        }),
        Err(unparseable) => {
            violations.push(CompatibilityViolation::UnparseableVersion(unparseable))
        }
    }

    match channel {
        Some(channel) if violations.is_empty() => Ok(AdmittedRelease {
            channel,
            release: candidate.release.clone(),
            migration_required,
        }),
        _ => Err(violations),
    }
}

/// Convenience entry point for a caller that already holds a schema-valid,
/// typed [`ReleaseChannelV1`] (e.g. one just deserialized successfully by
/// `serde`). Runs through the exact same [`evaluate_compatibility`] path as
/// an untrusted wire value -- there is no separate "trusted" fast path that
/// could silently admit something the raw path would refuse.
pub fn evaluate_release_channel(
    current_version: &str,
    candidate: &ReleaseChannelV1,
) -> Result<AdmittedRelease, Vec<CompatibilityViolation>> {
    evaluate_compatibility(current_version, &RawReleaseDescriptor::from(candidate))
}

fn parse_channel(raw: &str) -> Result<ReleaseChannel, CompatibilityViolation> {
    match raw {
        "stable" => Ok(ReleaseChannel::Stable),
        "beta" => Ok(ReleaseChannel::Beta),
        "nightly" => Ok(ReleaseChannel::Nightly),
        other => Err(CompatibilityViolation::UnknownChannel(other.to_string())),
    }
}

fn parse_schema_compatibility(raw: &str) -> Result<SchemaCompatibility, CompatibilityViolation> {
    match raw {
        "compatible" => Ok(SchemaCompatibility::Compatible),
        "migration_required" => Ok(SchemaCompatibility::MigrationRequired),
        "incompatible" => Ok(SchemaCompatibility::Incompatible),
        "unknown" => Ok(SchemaCompatibility::Unknown),
        other => Err(CompatibilityViolation::UnknownSchemaCompatibility(
            other.to_string(),
        )),
    }
}

/// Same strictly-greater-triple upgrade rule as membrane-updater's
/// `is_upgrade` (`engine/crates/membrane-updater/src/lib.rs`): `to` must be
/// a strictly greater `major.minor.patch` than `from`. `Ok(true)` is a real
/// upgrade; `Ok(false)` is equal-or-lower (fail closed, refused as a
/// downgrade); `Err(the offending version string)` means that version could
/// not be parsed at all, which is also fail closed but reported as its own
/// [`CompatibilityViolation::UnparseableVersion`] rather than folded into
/// `Downgrade`, so the caller knows which of the two closely related
/// failures it is looking at.
fn is_strict_upgrade(current: &str, candidate: &str) -> Result<bool, String> {
    match (version_triple(current), version_triple(candidate)) {
        (Some(from), Some(to)) => Ok(to > from),
        (None, _) => Err(current.to_string()),
        (_, None) => Err(candidate.to_string()),
    }
}

/// Parses the leading `major.minor.patch` numeric triple from a version
/// string, tolerating a pre-release/build suffix on the patch component
/// (e.g. `2.1.0-rc.1`) -- identical behavior to membrane-updater's
/// `version_triple`.
fn version_triple(version: &str) -> Option<(u64, u64, u64)> {
    let mut parts = version.split('.');
    let major = leading_digits(parts.next()?)?;
    let minor = leading_digits(parts.next()?)?;
    let patch = leading_digits(parts.next()?)?;
    Some((major, minor, patch))
}

fn leading_digits(part: &str) -> Option<u64> {
    let digits: String = part.chars().take_while(char::is_ascii_digit).collect();
    if digits.is_empty() {
        None
    } else {
        digits.parse().ok()
    }
}

// --- Deterministic, unexecuted-at-task-time source tests -------------------
//
// These `#[test]`s are NOT run by this task (per MEMBRANE-BOOK-MODE-EXECUTION-RULES.md
// and this task's hard rules: no `cargo test`). They are reasoned through by
// type and left for `cargo test --manifest-path engine/Cargo.toml --workspace`
// at the Book 3 final phased gate. Each isolates exactly one of the
// fail-closed cases MBR-912 names, plus the admit paths and the multi-violation
// case.
#[cfg(test)]
mod tests {
    use super::*;
    use crate::release_channel::SupportWindowV1;

    fn descriptor(
        channel: &str,
        release: &str,
        schema_version: u32,
        schema_compatibility: &str,
    ) -> RawReleaseDescriptor {
        RawReleaseDescriptor {
            schema_version,
            channel: channel.to_string(),
            release: release.to_string(),
            schema_compatibility: schema_compatibility.to_string(),
        }
    }

    #[test]
    fn unknown_channel_is_refused() {
        let candidate = descriptor("canary", "1.1.0", 1, "compatible");
        let violations = evaluate_compatibility("1.0.0", &candidate)
            .expect_err("unknown channel must fail closed");
        assert!(violations.contains(&CompatibilityViolation::UnknownChannel(
            "canary".to_string()
        )));
    }

    #[test]
    fn unsupported_schema_version_is_refused() {
        let candidate = descriptor("stable", "1.1.0", 2, "compatible");
        let violations = evaluate_compatibility("1.0.0", &candidate)
            .expect_err("unsupported schema version must fail closed");
        assert!(violations.contains(&CompatibilityViolation::UnsupportedSchemaVersion(2)));
    }

    #[test]
    fn unknown_schema_compatibility_string_is_refused() {
        let candidate = descriptor("stable", "1.1.0", 1, "maybe");
        let violations = evaluate_compatibility("1.0.0", &candidate)
            .expect_err("an unrecognized schemaCompatibility string must fail closed");
        assert!(
            violations.contains(&CompatibilityViolation::UnknownSchemaCompatibility(
                "maybe".to_string()
            ))
        );
    }

    #[test]
    fn explicit_unknown_schema_compatibility_is_refused_not_assumed_compatible() {
        let candidate = descriptor("stable", "1.1.0", 1, "unknown");
        let violations = evaluate_compatibility("1.0.0", &candidate)
            .expect_err("schemaCompatibility=unknown must never be treated as compatible");
        assert!(
            violations.contains(&CompatibilityViolation::UnknownSchemaCompatibility(
                "unknown".to_string()
            ))
        );
    }

    #[test]
    fn incompatible_schema_is_refused() {
        let candidate = descriptor("stable", "1.1.0", 1, "incompatible");
        let violations = evaluate_compatibility("1.0.0", &candidate)
            .expect_err("incompatible schema must fail closed");
        assert_eq!(violations, vec![CompatibilityViolation::IncompatibleSchema]);
    }

    #[test]
    fn downgrade_is_refused() {
        let candidate = descriptor("stable", "0.9.0", 1, "compatible");
        let violations =
            evaluate_compatibility("1.0.0", &candidate).expect_err("a downgrade must fail closed");
        assert!(violations.contains(&CompatibilityViolation::Downgrade {
            current: "1.0.0".to_string(),
            candidate: "0.9.0".to_string(),
        }));
    }

    #[test]
    fn equal_version_is_refused_not_an_upgrade() {
        let candidate = descriptor("stable", "1.0.0", 1, "compatible");
        let violations = evaluate_compatibility("1.0.0", &candidate)
            .expect_err("an equal version is not an upgrade");
        assert!(violations
            .iter()
            .any(|v| matches!(v, CompatibilityViolation::Downgrade { .. })));
    }

    #[test]
    fn unparseable_candidate_version_is_refused() {
        let candidate = descriptor("stable", "latest", 1, "compatible");
        let violations = evaluate_compatibility("1.0.0", &candidate)
            .expect_err("an unparseable candidate version must fail closed");
        assert!(
            violations.contains(&CompatibilityViolation::UnparseableVersion(
                "latest".to_string()
            ))
        );
    }

    #[test]
    fn unparseable_current_version_is_refused() {
        let candidate = descriptor("stable", "1.1.0", 1, "compatible");
        let violations = evaluate_compatibility("not-a-version", &candidate)
            .expect_err("an unparseable local version must also fail closed");
        assert!(
            violations.contains(&CompatibilityViolation::UnparseableVersion(
                "not-a-version".to_string()
            ))
        );
    }

    #[test]
    fn multiple_simultaneous_violations_are_all_reported() {
        let candidate = descriptor("canary", "0.9.0", 2, "incompatible");
        let violations = evaluate_compatibility("1.0.0", &candidate)
            .expect_err("every independent violation must be collected");
        assert_eq!(
            violations.len(),
            4,
            "unknown channel + unsupported schema version + incompatible schema + downgrade"
        );
    }

    #[test]
    fn compatible_upgrade_is_admitted() {
        let candidate = descriptor("stable", "1.1.0", 1, "compatible");
        let admitted = evaluate_compatibility("1.0.0", &candidate)
            .expect("a real upgrade on a known channel with compatible schema must be admitted");
        assert_eq!(admitted.channel, ReleaseChannel::Stable);
        assert_eq!(admitted.release, "1.1.0");
        assert!(!admitted.migration_required);
    }

    #[test]
    fn migration_required_upgrade_is_admitted_and_flagged() {
        let candidate = descriptor("beta", "2.0.0", 1, "migration_required");
        let admitted = evaluate_compatibility("1.9.0", &candidate)
            .expect("migration_required is admissible, not a refusal");
        assert_eq!(admitted.channel, ReleaseChannel::Beta);
        assert!(
            admitted.migration_required,
            "the caller must still see that a migration step is required before activation"
        );
    }

    #[test]
    fn evaluate_release_channel_wraps_a_typed_descriptor_through_the_same_path() {
        let typed = ReleaseChannelV1 {
            schema_version: RELEASE_CHANNEL_SCHEMA_VERSION,
            channel: ReleaseChannel::Nightly,
            release: "1.2.0-nightly.3".to_string(),
            support: SupportWindowV1 {
                state: crate::release_channel::SupportState::Supported,
                starts_at: "2026-01-01".to_string(),
                ends_at: None,
            },
            schema_compatibility: SchemaCompatibility::Compatible,
            migration: "no migration required".to_string(),
            rollback: "restore prior release".to_string(),
            signed_update_evidence: Some("sha256:signed-evidence-placeholder".to_string()),
        };
        let admitted = evaluate_release_channel("1.1.0", &typed)
            .expect("a typed, schema-valid descriptor with a real upgrade must be admitted");
        assert_eq!(admitted.channel, ReleaseChannel::Nightly);
        assert_eq!(admitted.release, "1.2.0-nightly.3");
    }

    #[test]
    fn as_str_round_trips_every_channel_and_compatibility_variant() {
        for (channel, raw) in [
            (ReleaseChannel::Stable, "stable"),
            (ReleaseChannel::Beta, "beta"),
            (ReleaseChannel::Nightly, "nightly"),
        ] {
            assert_eq!(channel.as_str(), raw);
            assert_eq!(parse_channel(raw), Ok(channel));
        }
        for (compat, raw) in [
            (SchemaCompatibility::Compatible, "compatible"),
            (SchemaCompatibility::MigrationRequired, "migration_required"),
            (SchemaCompatibility::Incompatible, "incompatible"),
            (SchemaCompatibility::Unknown, "unknown"),
        ] {
            assert_eq!(compat.as_str(), raw);
            assert_eq!(parse_schema_compatibility(raw), Ok(compat));
        }
    }
}
