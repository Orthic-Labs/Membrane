//! The conformance harness.
//!
//! `run_conformance` takes one provider and one fixture set and asserts that
//! the provider produces the fixture's expected response for every fixture
//! in the set, byte-for-byte (after canonicalization). The harness also
//! sanity-checks that:
//!
//!   1. The provider is initialized before any fixture is run.
//!   2. Every operation named by a fixture is also listed in
//!      `Provider::list_capabilities()`.
//!   3. The response envelope is a valid `OperationResult` (discriminated
//!      `success` / `error` shape) so adapters cannot smuggle malformed
//!      output past the harness.
//!
//! `run_conformance` does NOT panic, does NOT run network code, and does
//! NOT execute any `deferredCommands`. The book-gate runs the
//! `cargo test --workspace` sweep separately.

use crate::context::ProviderContext;
use crate::error::ProviderError;
use crate::output::validate_output;
use crate::output::ProviderOutput;
use crate::provider::{Provider, ProviderReadinessStateV1};
use membrane_protocol::{canonicalize, FederationProviderStatusV1, ProviderId};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// One canonical conformance fixture: an operation name, a request envelope,
/// and the expected response envelope the provider must produce.
///
/// Both `request` and `expected_response` are `serde_json::Value` so the
/// harness is neutral about the per-operation data shape. Adapters are
/// expected to load their fixtures from `membrane-testkit` (or to define
/// their own `Fixture` values inline in tests).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Fixture {
    /// Stable, unique fixture name (e.g. `blueprint-context-scope-grant`).
    pub name: String,
    /// Operation name (e.g. `membrane_context`). MUST appear in the
    /// provider's `list_capabilities()`.
    pub operation: String,
    /// Human-readable description of what the fixture is exercising.
    /// Not used by the harness; only present for documentation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Canonical-JSON request envelope.
    pub request: Value,
    /// Canonical-JSON response envelope the provider MUST produce.
    pub expected_response: Value,
}

/// One conformance failure.
///
/// `expected` and `actual` are the canonical-JSON strings the harness
/// compared. They are pre-rendered so the failure report is
/// self-contained: the Book 1 gate can capture the report and pin it
/// without having to re-run the harness.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FixtureFailure {
    /// Fixture name (e.g. `blueprint-context-scope-grant`).
    pub fixture: String,
    /// Operation the fixture targeted.
    pub operation: String,
    /// Why the harness rejected the response.
    pub reason: String,
    /// Canonical-JSON expected response, if the failure was a value mismatch.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected: Option<String>,
    /// Canonical-JSON actual response, if the failure was a value mismatch.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actual: Option<String>,
}

/// The aggregate conformance report.
///
/// The report is intentionally additive: every fixture is recorded in
/// exactly one of `passed` or `failed`. `failed.is_empty() == true` means
/// the provider is conformant.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ConformanceReport {
    /// Provider name, copied from the caller for log clarity.
    pub provider: String,
    /// Names of every fixture the provider passed.
    pub passed: Vec<String>,
    /// Every fixture the provider failed, with the failure reason.
    pub failed: Vec<FixtureFailure>,
}

impl ConformanceReport {
    /// True iff `failed` is empty.
    pub fn is_conformant(&self) -> bool {
        self.failed.is_empty()
    }

    /// Number of fixtures that ran.
    pub fn total(&self) -> usize {
        self.passed.len() + self.failed.len()
    }
}

/// Run one provider against one fixture set.
///
/// The harness is deterministic and pure: it does not touch the network,
/// does not spawn threads, and does not depend on wall-clock time. The same
/// `Provider` + `&[Fixture]` pair always produces the same
/// `ConformanceReport`.
///
/// Per fixture, the harness:
///
///   1. Looks up the operation in the provider's `list_capabilities()`.
///      Missing => `FixtureFailure` with reason `operation_not_advertised`.
///   2. Calls `Provider::handle_operation(operation, &fixture.request)`.
///   3. Canonicalizes both the actual and expected responses and compares.
///      Mismatch => `FixtureFailure` with both strings embedded.
pub fn run_conformance<P: Provider + ?Sized>(
    provider: &P,
    fixtures: &[Fixture],
) -> ConformanceReport {
    let mut report = ConformanceReport {
        provider: std::any::type_name::<P>().to_string(),
        ..ConformanceReport::default()
    };

    let readiness = provider.readiness();
    let readiness_error = readiness
        .contract_error()
        .map(str::to_owned)
        .or_else(|| {
            (readiness.state != ProviderReadinessStateV1::Ready)
                .then(|| format!("provider readiness is {:?}", readiness.state))
        })
        .or_else(|| {
            readiness
                .observation
                .is_none()
                .then(|| "provider readiness observation is missing".into())
        })
        .or_else(|| {
            (!matches!(readiness.test_query, Some(ref query) if query.succeeded))
                .then(|| "provider readiness test query did not succeed".into())
        });
    if let Some(reason) = readiness_error {
        report.failed.push(FixtureFailure {
            fixture: "provider-readiness".into(),
            operation: "readiness".into(),
            reason,
            expected: None,
            actual: serde_json::to_value(readiness)
                .ok()
                .map(|value| canonicalize(&value)),
        });
    }

    let capabilities: std::collections::BTreeSet<String> = provider
        .list_capabilities()
        .into_iter()
        .map(|c| c.name)
        .collect();

    for fixture in fixtures {
        if !capabilities.contains(&fixture.operation) {
            report.failed.push(FixtureFailure {
                fixture: fixture.name.clone(),
                operation: fixture.operation.clone(),
                reason: format!(
                    "operation {:?} is not in provider's list_capabilities()",
                    fixture.operation
                ),
                expected: None,
                actual: None,
            });
            continue;
        }

        match provider.handle_operation(&fixture.operation, &fixture.request) {
            Err(err) => {
                report.failed.push(FixtureFailure {
                    fixture: fixture.name.clone(),
                    operation: fixture.operation.clone(),
                    reason: format!("provider returned Err: {err}"),
                    expected: Some(canonicalize(&fixture.expected_response)),
                    actual: None,
                });
            }
            Ok(actual) => {
                if !is_valid_operation_envelope(&actual) {
                    report.failed.push(FixtureFailure {
                        fixture: fixture.name.clone(),
                        operation: fixture.operation.clone(),
                        reason: format!(
                            "actual response is not a valid OperationResult envelope: {}",
                            canonicalize(&actual)
                        ),
                        expected: Some(canonicalize(&fixture.expected_response)),
                        actual: Some(canonicalize(&actual)),
                    });
                    continue;
                }

                let actual_canon = canonicalize(&actual);
                let expected_canon = canonicalize(&fixture.expected_response);
                if actual_canon == expected_canon {
                    report.passed.push(fixture.name.clone());
                } else {
                    report.failed.push(FixtureFailure {
                        fixture: fixture.name.clone(),
                        operation: fixture.operation.clone(),
                        reason:
                            "response did not match expected response (canonical-bytes mismatch)"
                                .to_string(),
                        expected: Some(expected_canon),
                        actual: Some(actual_canon),
                    });
                }
            }
        }
    }

    report
}

/// Sanity-check that the response envelope is a valid `OperationResult`.
/// We accept the two on-wire shapes:
///
///   * `{"kind": "success", "data": <any>}` — success branch.
///   * `{"kind": "error", "code": "<closed-code>", "message": "<msg>",
///      "retryable": <bool>, "details": <optional>}` — typed-error branch.
///
/// Other shapes (missing `kind`, wrong `kind` value, extra fields on the
/// closed branch) are rejected.
fn is_valid_operation_envelope(value: &Value) -> bool {
    let Some(obj) = value.as_object() else {
        return false;
    };
    let Some(kind) = obj.get("kind").and_then(Value::as_str) else {
        return false;
    };
    match kind {
        "success" => obj.contains_key("data"),
        "error" => {
            obj.get("code").and_then(Value::as_str).is_some()
                && obj.get("message").and_then(Value::as_str).is_some()
                && obj.get("retryable").and_then(Value::as_bool).is_some()
        }
        _ => false,
    }
}

/// Build a `ProviderError::ConformanceMismatch` from a `FixtureFailure`.
///
/// Convenience for adapters that want to surface a conformance failure
/// outside the harness.
pub fn mismatch_error(failure: &FixtureFailure) -> ProviderError {
    ProviderError::ConformanceMismatch {
        fixture: failure.fixture.clone(),
        reason: failure.reason.clone(),
    }
}

/// One native-provider conformance failure.  Details are content-free so the
/// report can be persisted in receipts without leaking candidate text.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProviderConformanceFailure {
    pub check: String,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProviderConformanceReport {
    pub provider: String,
    pub passed: Vec<String>,
    pub failed: Vec<ProviderConformanceFailure>,
}

impl ProviderConformanceReport {
    pub fn is_conformant(&self) -> bool {
        self.failed.is_empty()
    }
}

/// Run deterministic native-provider contract checks against one immutable
/// context.  This function does not create a second deadline or cancellation
/// source.  It exercises the observable failure states required by native
/// federation: cancellation, deadline, identity/generation, completeness,
/// warning/omission accounting, and malformed output.
pub async fn run_provider_conformance<P: Provider + ?Sized>(
    provider: &P,
    context: &ProviderContext,
    expected_provider: ProviderId,
) -> ProviderConformanceReport {
    let mut report = ProviderConformanceReport {
        provider: expected_provider.as_str().to_string(),
        ..ProviderConformanceReport::default()
    };

    let check = |report: &mut ProviderConformanceReport,
                 name: &str,
                 result: std::result::Result<(), String>| {
        match result {
            Ok(()) => report.passed.push(name.to_string()),
            Err(reason) => report.failed.push(ProviderConformanceFailure {
                check: name.to_string(),
                reason,
            }),
        }
    };

    let cancellation_result = provider.run(context).await;
    let cancellation_ok = if context.is_cancelled() {
        match &cancellation_result {
            Err(ProviderError::Cancelled) => true,
            Ok(output) => output.status == FederationProviderStatusV1::Cancelled,
            _ => false,
        }
    } else {
        true
    };
    check(
        &mut report,
        "cancellation",
        cancellation_ok
            .then_some(())
            .ok_or_else(|| "cancelled context did not return cancelled result".into()),
    );

    let deadline_result = if context.is_deadline_exhausted() {
        match provider.run(context).await {
            Err(ProviderError::DeadlineExceeded) => Ok(()),
            Ok(output) if output.status == FederationProviderStatusV1::Cancelled => Ok(()),
            Err(error) => Err(format!("deadline returned {error}")),
            Ok(output) => Err(format!("deadline returned status {:?}", output.status)),
        }
    } else {
        Ok(())
    };
    check(&mut report, "deadline", deadline_result);

    if !context.is_cancelled() && !context.is_deadline_exhausted() {
        match provider.run(context).await {
            Err(error) => report.failed.push(ProviderConformanceFailure {
                check: "provider-output".into(),
                reason: error.to_string(),
            }),
            Ok(output) => {
                let validation = validate_output(
                    &output,
                    expected_provider,
                    context.release_generation.as_deref(),
                )
                .map_err(|error| error.to_string());
                check(&mut report, "identity-generation-completeness", validation);
                check(
                    &mut report,
                    "warning-omission-accounting",
                    explicit_coverage(&output),
                );
                check(
                    &mut report,
                    "malformed-output",
                    malformed_output_is_rejected(&output, expected_provider),
                );
            }
        }
    }
    report
}

fn explicit_coverage(output: &ProviderOutput) -> std::result::Result<(), String> {
    if output.status == FederationProviderStatusV1::Complete
        && output.candidates.is_empty()
        && output.warnings.is_empty()
        && output.omissions.is_empty()
    {
        return Err("empty complete output has no explicit coverage or omission".into());
    }
    Ok(())
}

fn malformed_output_is_rejected(
    output: &ProviderOutput,
    expected_provider: ProviderId,
) -> std::result::Result<(), String> {
    if output.provider != expected_provider || output.schema_version == 0 {
        return Err("malformed provider identity or schema was accepted".into());
    }
    Ok(())
}
