//! MEM-036: `POST /diagnostics/fence/evaluate` exercised end-to-end against a
//! REAL sealed mutation epoch — no fabricated snapshots, no canned verdicts.
//!
//! Every workspace epoch here is admitted through the real
//! `DiagnosticsService` boundary (`workspace.open` → `mutation.begin` →
//! `mutation.seal` / `mutation.registerObserved`) after real bytes are written
//! under the bound project root and hashed. The evidence snapshot under test
//! is the one the service itself assembled for that sealed epoch
//! (`snapshot.await` → `snapshot.get`), and the verdicts come from the same
//! evaluator the HTTP route and the native-pipe dispatch call
//! (`DiagnosticsService::evaluate_fence` → `evaluate_gate`).
//!
//! Verdict vocabulary (design `docs/architecture/live-diagnostics.md` §5.2,
//! `GateOutcome`): `clean_exact` is the only allow verdict, `dirty_exact` is
//! the proven block verdict, and `unknown_*` / `superseded` are degraded
//! verdicts that never clear the fence. Service-level failures are typed
//! omission envelopes (`no_sealed_epoch`, `provider_error`,
//! `invalid_request`), never verdicts — these tests assert both layers keep
//! that separation.
//!
//! Determinism: `snapshot.await` is driven with `maxCost: instant`, so the
//! real D1 engines (typescript-native-d1 / rust-analyzer-native-d1, both
//! `interactive` cost) are excluded even on machines where their binaries
//! resolve — they can only add a `provider_exceeds_max_cost` omission, never
//! a lane. The Blueprint D0 lane is reached through the documented
//! `with_blueprint_client` seam with a client that hashes the real workspace
//! files at fetch time; the service independently re-verifies those hashes
//! against the sealed epoch before admitting an exact lane, so the double
//! cannot fabricate clean evidence.

use axum::body::{to_bytes, Body};
use axum::http::{header, Request, StatusCode};
use axum::Router;
use membrane_protocol::diagnostics::{
    BlueprintFreshness, CapabilityVocabulary, ChangedFileHashV1, ConvergenceClass, CostClass,
    DiagnosticEvidenceSnapshotV1, DiagnosticGateDecisionV1, GateOutcome, GatePolicyProfileV1,
    ObligationState, WorkspaceEpochOrigin, WorkspaceEpochV1,
    DIAGNOSTIC_EVIDENCE_SNAPSHOT_SCHEMA_VERSION, DIAGNOSTIC_GATE_DECISION_SCHEMA_VERSION,
    WORKSPACE_EPOCH_SCHEMA_VERSION,
};
use membrane_protocol::CanonicalSerialize;
use membrane_runtime::live_diagnostics_service::{
    DiagnosticsService, SnapshotAwaitRequest, AUDIT_RELATIVE_PATH, DEFAULT_POLICY_PROFILE_NAME,
};
use membrane_runtime::native_diagnostics_pipe::NATIVE_DIAGNOSTICS_PIPE_SCHEMA_VERSION;
use membrane_runtime::providers::blueprint_findings::{
    BlueprintFinding, BlueprintFindingsClient, BlueprintFindingsError, BlueprintFindingsResult,
};
use membrane_runtime::{
    diagnostics_native_dispatch, diagnostics_router, FenceEvaluateRequest, NativeDiagnosticsRequest,
};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::path::Path;
use std::sync::{Arc, Mutex, MutexGuard};
use tempfile::TempDir;
use tower::ServiceExt;

const REPO: &str = "repo-fence-eval";
const WORKTREE: &str = "wt-fence-eval";
/// Repo-relative path of the file the mutation actually rewrites on disk.
const TOUCHED: &str = "docs/notes.md";
const MAX_HTTP_BODY: usize = 4 * 1024 * 1024;

/// `sha256:<hex>` of real bytes — the digest form the contracts use.
fn sha256_hex(bytes: &[u8]) -> String {
    format!("sha256:{}", hex::encode(Sha256::digest(bytes)))
}

/// Write real contents under `root` and return the content hash a host binds
/// into the epoch. The epoch therefore describes bytes that actually exist.
fn write_real_file(root: &Path, rel: &str, contents: &str) -> String {
    let path = root.join(rel);
    std::fs::create_dir_all(path.parent().expect("touched file has a parent"))
        .expect("create touched-file parent");
    std::fs::write(&path, contents).expect("write real workspace file");
    sha256_hex(&std::fs::read(&path).expect("read back real workspace file"))
}

/// The manifest digest a host computes over the resulting tree: sha256 over
/// the sorted `path=hash` manifest lines. A real function of real inputs.
fn manifest_digest(files: &[(String, String)]) -> String {
    let mut lines: Vec<String> = files
        .iter()
        .map(|(path, hash)| format!("{path}={hash}"))
        .collect();
    lines.sort();
    sha256_hex(lines.join("\n").as_bytes())
}

/// Host-constructed `WorkspaceEpochV1` describing real resulting bytes. The
/// service still validates identity, monotonicity, the parent chain, and
/// project-root confinement before admitting it — nothing here bypasses a
/// check.
fn sealed_epoch(
    number: u64,
    files: &[(String, String)],
    origin: WorkspaceEpochOrigin,
) -> WorkspaceEpochV1 {
    WorkspaceEpochV1 {
        schema_version: WORKSPACE_EPOCH_SCHEMA_VERSION.to_string(),
        repo_id: REPO.to_string(),
        worktree_id: WORKTREE.to_string(),
        epoch: number,
        parent_epoch: if number > 1 { Some(number - 1) } else { None },
        mutation_id: Some(format!("mutation-{number}")),
        source_manifest_digest: manifest_digest(files),
        changed_paths: files.iter().map(|(path, _)| path.clone()).collect(),
        changed_file_hashes: files
            .iter()
            .map(|(path, hash)| ChangedFileHashV1 {
                path: path.clone(),
                hash: hash.clone(),
            })
            .collect(),
        project_config_digest: sha256_hex(b"membrane fence-eval project configuration"),
        toolchain_digest: sha256_hex(b"membrane fence-eval toolchain"),
        sandbox_policy_digest: sha256_hex(b"membrane fence-eval sandbox policy"),
        origin,
    }
}

/// A `BlueprintFindingsClient` double that reads the real workspace bytes it
/// is asked about: `per_file_hashes` are sha256 of the files as they exist
/// under `repo_root` at fetch time, and `generation_id` is derived from that
/// exact content set. The service re-verifies every retained hash against the
/// sealed epoch before admitting an exact D0 lane, so this double cannot
/// fabricate clean evidence — an invented hash fails closed. What it does not
/// do is speak the daemon transport; `with_blueprint_client` is the documented
/// seam for exactly that substitution.
struct RealFilesFindings {
    /// Findings to report: `(rule_id, path, severity)` triples about the real
    /// sealed file. `severity: "error"` maps to a blocking observation through
    /// the real `BlueprintFinding::to_observation`.
    findings: Vec<(String, String, String)>,
}

impl BlueprintFindingsClient for RealFilesFindings {
    fn fetch(
        &mut self,
        repo_root: &Path,
        _timeout_ms: u64,
        paths: &[String],
    ) -> Result<BlueprintFindingsResult, BlueprintFindingsError> {
        let mut per_file_hashes = BTreeMap::new();
        let mut material = Vec::new();
        for rel in paths {
            let full = repo_root.join(rel);
            let bytes = std::fs::read(&full).map_err(|error| {
                BlueprintFindingsError::GraphMissing(format!(
                    "{} unreadable in workspace: {error}",
                    full.display()
                ))
            })?;
            let hash = sha256_hex(&bytes);
            material.push(format!("{rel}={hash}"));
            per_file_hashes.insert(rel.clone(), hash);
        }
        material.sort();
        let findings = self
            .findings
            .iter()
            .map(|(rule_id, path, severity)| BlueprintFinding {
                rule_id: rule_id.clone(),
                path: path.clone(),
                start_line: Some(1),
                end_line: Some(1),
                name: Some(format!("{rule_id}Symbol")),
                specifier: None,
                severity: Some(severity.clone()),
                fingerprint: sha256_hex(format!("{rule_id}|{path}|{severity}").as_bytes()),
            })
            .collect();
        Ok(BlueprintFindingsResult {
            generation_id: sha256_hex(material.join("\n").as_bytes()),
            freshness: "current".to_string(),
            findings,
            omissions: Vec::new(),
            per_file_hashes,
        })
    }
}

/// Byte-identical reproduction of the effective policy `snapshot.await`
/// resolves for the seeded `changed-files-zero` profile plus an explicit
/// request capability set (see `DiagnosticsService::resolve_policy`: request
/// capabilities win, then `policyDigest` is cleared and re-digested
/// canonically). Reusing the resolved policy means `fence.evaluate` must
/// reproduce the acquisition decision exactly.
fn resolved_policy(required: &[CapabilityVocabulary]) -> GatePolicyProfileV1 {
    let mut policy = GatePolicyProfileV1 {
        profile_name: DEFAULT_POLICY_PROFILE_NAME.to_string(),
        policy_version: "v1".to_string(),
        policy_digest: String::new(),
        blocking_codes: Vec::new(),
        required_capabilities: required.to_vec(),
    };
    policy.policy_digest = policy.canonical_digest();
    policy
}

fn await_request(required: &[CapabilityVocabulary]) -> SnapshotAwaitRequest {
    SnapshotAwaitRequest {
        repo_id: REPO.to_string(),
        worktree_id: WORKTREE.to_string(),
        policy_profile_name: DEFAULT_POLICY_PROFILE_NAME.to_string(),
        required_capabilities: required.to_vec(),
        // Instant ceiling: the only in-scope evidence is the Blueprint D0
        // lane (Instant). Real D1 engines are Interactive and therefore
        // excluded on every machine, installed or not.
        max_cost: Some(CostClass::Instant),
        deadline_ms: Some(5_000),
    }
}

struct Fixture {
    data_root: TempDir,
    project: TempDir,
    service: Arc<Mutex<DiagnosticsService>>,
}

impl Fixture {
    fn new(client: Option<RealFilesFindings>) -> Self {
        let data_root = tempfile::tempdir().expect("data root");
        let project = tempfile::tempdir().expect("project root");
        let mut service =
            DiagnosticsService::with_data_root(data_root.path().to_path_buf()).expect("service");
        if let Some(client) = client {
            service = service.with_blueprint_client(Box::new(client));
        }
        Self {
            data_root,
            project,
            service: Arc::new(Mutex::new(service)),
        }
    }

    fn lock(&self) -> MutexGuard<'_, DiagnosticsService> {
        self.service.lock().expect("diagnostics service lock")
    }

    fn router(&self) -> Router {
        diagnostics_router(Arc::clone(&self.service))
    }

    fn open(&self) {
        let status = self
            .lock()
            .workspace_open(REPO, WORKTREE, Some(self.project.path().to_str().unwrap()))
            .expect("workspace opens against the real project root");
        assert_eq!(status["created"], json!(true));
    }

    /// Open → begin → seal one real epoch built over real on-disk bytes.
    /// Returns the admitted epoch for use as `expectedEpoch`.
    fn open_begin_seal(&self, contents: &str, number: u64) -> WorkspaceEpochV1 {
        self.open();
        let hash = write_real_file(self.project.path(), TOUCHED, contents);
        let files = vec![(TOUCHED.to_string(), hash)];
        let epoch = sealed_epoch(number, &files, WorkspaceEpochOrigin::Transactional);
        self.lock()
            .mutation_begin(REPO, WORKTREE)
            .expect("mutation begins");
        let seal = self
            .lock()
            .mutation_seal(REPO, WORKTREE, epoch.clone())
            .expect("real epoch seals");
        assert_eq!(seal["sealedEpoch"], json!(number));
        assert_eq!(seal["fenceCleared"], json!(false));
        epoch
    }

    fn await_decision(&self, required: &[CapabilityVocabulary]) -> DiagnosticGateDecisionV1 {
        self.lock()
            .snapshot_await(&await_request(required))
            .expect("snapshot acquisition over the sealed epoch succeeds")
    }

    /// The real snapshot the service stored for the sealed epoch.
    fn latest_snapshot(&self) -> DiagnosticEvidenceSnapshotV1 {
        let value = self
            .lock()
            .snapshot_get(REPO, WORKTREE)
            .expect("snapshot exists after acquisition");
        serde_json::from_value(value).expect("snapshot/get returns the protocol snapshot shape")
    }

    fn audit_kinds(&self) -> Vec<String> {
        std::fs::read_to_string(self.data_root.path().join(AUDIT_RELATIVE_PATH))
            .unwrap_or_default()
            .lines()
            .filter(|line| !line.trim().is_empty())
            .map(|line| {
                serde_json::from_str::<Value>(line)
                    .expect("audit sink writes one JSON object per line")["kind"]
                    .as_str()
                    .expect("audit kind is a string")
                    .to_string()
            })
            .collect()
    }
}

async fn http_get(router: &Router, uri: &str) -> (StatusCode, Value) {
    let response = router
        .clone()
        .oneshot(Request::get(uri).body(Body::empty()).expect("request"))
        .await
        .expect("router response");
    let status = response.status();
    let bytes = to_bytes(response.into_body(), MAX_HTTP_BODY)
        .await
        .expect("response body");
    (status, serde_json::from_slice(&bytes).unwrap_or(Value::Null))
}

async fn http_post(router: &Router, uri: &str, body: Value) -> (StatusCode, Value) {
    let response = router
        .clone()
        .oneshot(
            Request::post(uri)
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(body.to_string()))
                .expect("request"),
        )
        .await
        .expect("router response");
    let status = response.status();
    let bytes = to_bytes(response.into_body(), MAX_HTTP_BODY)
        .await
        .expect("response body");
    (status, serde_json::from_slice(&bytes).unwrap_or(Value::Null))
}

/// The native-pipe dispatch mirror: same operation names, same request decode,
/// same evaluator — no loopback socket.
fn native_request(
    service: &Arc<Mutex<DiagnosticsService>>,
    method: &str,
    path: &str,
    query: Value,
    body: Value,
) -> (u16, Value) {
    let response = diagnostics_native_dispatch(
        service,
        NativeDiagnosticsRequest {
            schema_version: NATIVE_DIAGNOSTICS_PIPE_SCHEMA_VERSION,
            id: format!("test-{method}-{path}"),
            method: method.to_string(),
            path: path.to_string(),
            query,
            body,
            installation_id: String::new(),
            cortex_store_id: String::new(),
            release_generation: String::new(),
            service_generation: String::new(),
        },
    );
    (response.status, response.body)
}

/// Assert `decision` is a *typed verdict* bound to this exact snapshot/policy
/// pair — the evaluator echoes the snapshot id and policy identity — and that
/// the outcome obeys the documented §5.3 semantics for the evidence at hand
/// rather than a hardcoded expectation.
fn assert_typed_verdict(
    decision: &DiagnosticGateDecisionV1,
    snapshot: &DiagnosticEvidenceSnapshotV1,
    policy: &GatePolicyProfileV1,
) {
    assert_eq!(
        decision.schema_version, DIAGNOSTIC_GATE_DECISION_SCHEMA_VERSION,
        "verdict must carry the typed decision envelope"
    );
    assert_eq!(decision.snapshot_id, snapshot.snapshot_id);
    assert_eq!(decision.policy_profile, policy.profile_name);
    assert_eq!(decision.policy_version, policy.policy_version);
    assert_eq!(decision.policy_digest, policy.policy_digest);
    match decision.outcome {
        // §5.3 step 3: allow requires every required capability covered by a
        // complete exact lane bound to the snapshot's epoch and no obligation
        // left unsatisfied.
        GateOutcome::CleanExact => {
            assert!(decision.blocking_issue_ids.is_empty());
            assert!(
                snapshot
                    .coverage_obligations
                    .iter()
                    .all(|o| o.state == ObligationState::SatisfiedExact),
                "clean_exact with an unsatisfied obligation would be a gate violation"
            );
        }
        // §5.3 step 2: block verdicts name what blocked.
        GateOutcome::DirtyExact => {
            assert!(
                !decision.blocking_issue_ids.is_empty(),
                "dirty_exact must identify the blocking evidence"
            );
        }
        // Degraded verdicts are explicit about why nothing was proven.
        GateOutcome::UnknownIncomplete
        | GateOutcome::UnknownUnavailable
        | GateOutcome::UnknownTimedOut => {
            assert!(
                !decision.reason_codes.is_empty(),
                "unknown_* verdicts must carry reason codes"
            );
        }
        GateOutcome::UnknownConflict | GateOutcome::Superseded => {
            assert!(
                !decision.reason_codes.is_empty(),
                "conflict/superseded verdicts must carry reason codes"
            );
        }
    }
}

/// MEM-036 refusal coverage: nothing sealed, nothing acquired, unknown
/// provider digests, malformed wire bodies. Each must surface a typed
/// omission envelope — never a verdict payload.
#[tokio::test(flavor = "current_thread")]
async fn unsealed_workspace_and_missing_snapshot_fail_typed_not_verdict() {
    let fixture = Fixture::new(None);

    // Before workspace.open even the workspace identity is unknown.
    assert_eq!(
        fixture
            .lock()
            .workspace_status(REPO, WORKTREE)
            .unwrap_err()
            .code(),
        "workspace_not_open"
    );
    assert_eq!(
        fixture
            .lock()
            .snapshot_await(&await_request(&[CapabilityVocabulary::Syntax]))
            .unwrap_err()
            .code(),
        "workspace_not_open"
    );

    fixture.open();

    // Opened but unsealed: every snapshot surface refuses `no_sealed_epoch`.
    assert_eq!(
        fixture
            .lock()
            .snapshot_get(REPO, WORKTREE)
            .unwrap_err()
            .code(),
        "no_sealed_epoch"
    );
    assert_eq!(
        fixture
            .lock()
            .snapshot_explain(REPO, WORKTREE)
            .unwrap_err()
            .code(),
        "no_sealed_epoch"
    );
    assert_eq!(
        fixture
            .lock()
            .snapshot_delta(REPO, WORKTREE)
            .unwrap_err()
            .code(),
        "no_sealed_epoch"
    );
    assert_eq!(
        fixture
            .lock()
            .snapshot_await(&await_request(&[CapabilityVocabulary::Syntax]))
            .unwrap_err()
            .code(),
        "no_sealed_epoch"
    );

    // Sealing without an open mutation batch is a boundary violation.
    assert_eq!(
        fixture
            .lock()
            .mutation_seal(
                REPO,
                WORKTREE,
                sealed_epoch(1, &[], WorkspaceEpochOrigin::Transactional),
            )
            .unwrap_err()
            .code(),
        "mutation_boundary"
    );

    // Unknown provider digests are typed provider refusals, not verdicts.
    assert_eq!(
        fixture
            .lock()
            .provider_status("sha256:no-such-provider")
            .unwrap_err()
            .code(),
        "provider_error"
    );
    assert_eq!(
        fixture
            .lock()
            .provider_restart("sha256:no-such-provider")
            .unwrap_err()
            .code(),
        "provider_error"
    );

    // The same refusals hold over the real HTTP surface.
    let router = fixture.router();
    let (status, body) = http_get(
        &router,
        &format!("/diagnostics/snapshot/get?repoId={REPO}&worktreeId={WORKTREE}"),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(body["error"]["code"], json!("no_sealed_epoch"));
    assert!(
        body.get("outcome").is_none(),
        "a refusal envelope must not carry a verdict"
    );

    // A malformed evaluate body fails decode — typed invalid_request, not a
    // decision.
    let (status, body) = http_post(&router, "/diagnostics/fence/evaluate", json!({})).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"]["code"], json!("invalid_request"));

    // And over the native-pipe dispatch mirror.
    let (status, body) = native_request(
        &fixture.service,
        "GET",
        "/diagnostics/snapshot/get",
        json!({"repoId": REPO, "worktreeId": WORKTREE}),
        Value::Null,
    );
    assert_eq!(status, 409);
    assert_eq!(body["error"]["code"], json!("no_sealed_epoch"));
}

/// MEM-036 primary path: open → begin → seal → await → get →
/// `POST /diagnostics/fence/evaluate` on the real router, plus the
/// native-pipe mirror — all over one service instance and one real sealed
/// epoch. With no Blueprint client configured and no engine qualifying below
/// the Instant ceiling, the honest verdict for this sealed state is
/// `unknown_incomplete`: the Syntax obligation is provably uncovered. That is
/// the degraded typed verdict the sealed bytes actually earn — the fence must
/// not clear.
#[tokio::test(flavor = "current_thread")]
async fn sealed_epoch_fence_evaluate_returns_the_acquisition_verdict() {
    let fixture = Fixture::new(None);
    let epoch1 = fixture.open_begin_seal("# notes\nreal bytes v1\n", 1);

    // The sealed epoch identity is the one the session reports.
    let status = fixture.lock().workspace_status(REPO, WORKTREE).unwrap();
    assert_eq!(status["latestSealedEpoch"], json!(1));
    assert_eq!(
        status["manifestDigest"],
        json!(epoch1.source_manifest_digest)
    );
    assert_eq!(status["fenceCleared"], json!(false));

    // Sealed but never acquired: no snapshot exists yet — typed refusal, not
    // a fabricated empty snapshot.
    assert_eq!(
        fixture
            .lock()
            .snapshot_get(REPO, WORKTREE)
            .unwrap_err()
            .code(),
        "no_sealed_epoch"
    );

    // Acquire real evidence over the sealed epoch.
    let policy = resolved_policy(&[CapabilityVocabulary::Syntax]);
    let awaited = fixture.await_decision(&[CapabilityVocabulary::Syntax]);
    assert_eq!(
        awaited.outcome,
        GateOutcome::UnknownIncomplete,
        "with zero qualified lanes the sealed epoch must evaluate unknown_incomplete: {awaited:?}"
    );
    assert!(awaited
        .reason_codes
        .iter()
        .any(|code| code == "capability_uncovered:syntax"));
    assert!(awaited
        .omissions
        .iter()
        .any(|omission| omission.code == "blueprint_unavailable"));
    assert!(
        !fixture.lock().is_fence_cleared(REPO, WORKTREE),
        "a degraded verdict must never clear the fence"
    );

    // The stored snapshot is bound to the exact sealed epoch — this is the
    // real evidence envelope, not a fixture.
    let snapshot = fixture.latest_snapshot();
    assert_eq!(
        snapshot.schema_version,
        DIAGNOSTIC_EVIDENCE_SNAPSHOT_SCHEMA_VERSION
    );
    assert_eq!(
        snapshot.workspace_epoch, epoch1,
        "the snapshot must embed the exact sealed epoch"
    );
    assert_eq!(snapshot.blueprint_freshness, BlueprintFreshness::Unknown);

    // POST /diagnostics/fence/evaluate over the real axum router.
    let request = FenceEvaluateRequest {
        snapshot: snapshot.clone(),
        expected_epoch: epoch1.clone(),
        policy: policy.clone(),
    };
    let (status, body) = http_post(
        &fixture.router(),
        "/diagnostics/fence/evaluate",
        serde_json::to_value(&request).expect("request serializes"),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "evaluate must return a typed decision, not a refusal: {body}"
    );
    let evaluated: DiagnosticGateDecisionV1 =
        serde_json::from_value(body).expect("typed decision body");
    assert_typed_verdict(&evaluated, &snapshot, &policy);
    assert_eq!(evaluated.outcome, GateOutcome::UnknownIncomplete);
    // Identical inputs through the pure evaluator reproduce the acquisition
    // decision exactly — snapshot id, policy digest, outcome, reason codes.
    assert_eq!(
        evaluated, awaited,
        "fence.evaluate must reproduce the sealed-epoch acquisition verdict"
    );

    // The native-pipe dispatch mirror decodes the same request and must agree.
    let (status, body) = native_request(
        &fixture.service,
        "POST",
        "/diagnostics/fence/evaluate",
        Value::Null,
        serde_json::to_value(&request).expect("request serializes"),
    );
    assert_eq!(status, 200);
    let native_decision: DiagnosticGateDecisionV1 =
        serde_json::from_value(body).expect("native decision body");
    assert_eq!(native_decision, evaluated);

    // Explain reports the same degraded outcome against the same snapshot.
    let explain = fixture.lock().snapshot_explain(REPO, WORKTREE).unwrap();
    assert_eq!(explain["outcome"], json!("unknown_incomplete"));
    assert_eq!(explain["snapshotId"], json!(snapshot.snapshot_id));

    // The audit sink recorded the real seal and the real gate decision.
    let kinds = fixture.audit_kinds();
    for expected in ["epoch_sealed", "gate_decision"] {
        assert!(
            kinds.iter().any(|kind| kind == expected),
            "audit must record {expected}: {kinds:?}"
        );
    }
}

/// MEM-036 allow path: real Blueprint evidence whose per-file hashes match the
/// sealed epoch's bytes produces the only exact D0 lane, satisfies the Syntax
/// obligation, and the fence clears with `clean_exact`. A newer observed epoch
/// then invalidates the clearance, and the stale snapshot evaluated against it
/// is a typed `superseded` verdict — never an allow.
#[tokio::test(flavor = "current_thread")]
async fn verified_evidence_evaluates_clean_exact_then_new_epoch_supersedes() {
    let fixture = Fixture::new(Some(RealFilesFindings { findings: vec![] }));
    let epoch1 = fixture.open_begin_seal("# notes\nreal bytes v1\n", 1);

    let policy = resolved_policy(&[CapabilityVocabulary::Syntax]);
    let awaited = fixture.await_decision(&[CapabilityVocabulary::Syntax]);
    assert_eq!(
        awaited.outcome,
        GateOutcome::CleanExact,
        "verified exact D0 coverage of the required capability must clear: {awaited:?}"
    );
    assert!(fixture.lock().is_fence_cleared(REPO, WORKTREE));

    let snapshot = fixture.latest_snapshot();
    assert_eq!(snapshot.workspace_epoch, epoch1);
    assert_eq!(snapshot.blueprint_freshness, BlueprintFreshness::Current);
    assert!(
        snapshot.coverage_lanes.iter().any(|lane| lane.convergence_class
            == ConvergenceClass::SnapshotCheckerExact
            && lane.bound_workspace_epoch == 1
            && lane.state == membrane_protocol::diagnostics::LaneState::Complete),
        "a complete exact D0 lane bound to epoch 1 must be the clearing evidence"
    );

    // The HTTP evaluate endpoint reproduces the allow verdict.
    let request = FenceEvaluateRequest {
        snapshot: snapshot.clone(),
        expected_epoch: epoch1.clone(),
        policy: policy.clone(),
    };
    let (status, body) = http_post(
        &fixture.router(),
        "/diagnostics/fence/evaluate",
        serde_json::to_value(&request).expect("request serializes"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let evaluated: DiagnosticGateDecisionV1 =
        serde_json::from_value(body).expect("typed decision body");
    assert_typed_verdict(&evaluated, &snapshot, &policy);
    assert_eq!(evaluated.outcome, GateOutcome::CleanExact);
    assert_eq!(evaluated, awaited);

    // A newer observed epoch — real rewritten bytes, rehashed — invalidates
    // the clearance (stale-clearance invalidation path).
    let hash2 = write_real_file(fixture.project.path(), TOUCHED, "# notes\nreal bytes v2 changed\n");
    let files2 = vec![(TOUCHED.to_string(), hash2)];
    let epoch2 = sealed_epoch(2, &files2, WorkspaceEpochOrigin::ObservedHook);
    let observed = fixture
        .lock()
        .mutation_register_observed(REPO, WORKTREE, epoch2.clone())
        .expect("observed epoch registers");
    assert_eq!(observed["observedEpoch"], json!(2));
    assert_eq!(observed["fenceCleared"], json!(false));
    assert!(
        !fixture.lock().is_fence_cleared(REPO, WORKTREE),
        "newly sealed bytes must invalidate the epoch-1 clearance"
    );

    // Stale evidence against the now-current epoch: typed `superseded`, never
    // an allow — directly and over the wire.
    let stale = DiagnosticsService::evaluate_fence(&snapshot, &epoch2, &policy);
    assert_eq!(stale.outcome, GateOutcome::Superseded);
    assert_eq!(stale.reason_codes, vec!["superseded_epoch".to_string()]);

    let stale_request = FenceEvaluateRequest {
        snapshot: snapshot.clone(),
        expected_epoch: epoch2.clone(),
        policy: policy.clone(),
    };
    let (status, body) = http_post(
        &fixture.router(),
        "/diagnostics/fence/evaluate",
        serde_json::to_value(&stale_request).expect("request serializes"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let stale_evaluated: DiagnosticGateDecisionV1 =
        serde_json::from_value(body).expect("typed decision body");
    assert_eq!(stale_evaluated.outcome, GateOutcome::Superseded);

    // A conflicting manifest at the same epoch number and a foreign-worktree
    // epoch are typed conflict verdicts — never allow.
    let mut conflicted = epoch1.clone();
    conflicted.source_manifest_digest = "sha256:other-manifest".to_string();
    let conflict = DiagnosticsService::evaluate_fence(&snapshot, &conflicted, &policy);
    assert_eq!(conflict.outcome, GateOutcome::UnknownConflict);
    assert_eq!(conflict.reason_codes, vec!["manifest_conflict".to_string()]);

    let mut foreign = epoch1.clone();
    foreign.worktree_id = "wt-elsewhere".to_string();
    let foreign_decision = DiagnosticsService::evaluate_fence(&snapshot, &foreign, &policy);
    assert_eq!(foreign_decision.outcome, GateOutcome::UnknownConflict);
    assert_eq!(
        foreign_decision.reason_codes,
        vec!["identity_mismatch".to_string()]
    );
}

/// MEM-036 block path: a blocking Blueprint finding (`severity: "error"`)
/// over the sealed bytes flows through the real `to_observation` mapping into
/// a `severity_hint: blocking` observation; §5.3 step 2 proves `dirty_exact`
/// regardless of coverage, and the fence never clears.
#[tokio::test(flavor = "current_thread")]
async fn blocking_finding_over_sealed_bytes_evaluates_dirty_exact() {
    let fixture = Fixture::new(Some(RealFilesFindings {
        findings: vec![(
            "BP001".to_string(),
            TOUCHED.to_string(),
            "error".to_string(),
        )],
    }));
    let epoch1 = fixture.open_begin_seal("# notes\nbroken import\n", 1);

    let policy = resolved_policy(&[CapabilityVocabulary::Syntax]);
    let awaited = fixture.await_decision(&[CapabilityVocabulary::Syntax]);
    assert_eq!(
        awaited.outcome,
        GateOutcome::DirtyExact,
        "one blocking observation proves the block verdict: {awaited:?}"
    );
    assert_eq!(awaited.reason_codes, vec!["exact_blocker".to_string()]);
    assert!(!awaited.blocking_issue_ids.is_empty());
    assert!(
        !fixture.lock().is_fence_cleared(REPO, WORKTREE),
        "a proven-dirty epoch must never clear the fence"
    );

    // The same verdict through the native-pipe evaluate dispatch.
    let snapshot = fixture.latest_snapshot();
    let request = FenceEvaluateRequest {
        snapshot: snapshot.clone(),
        expected_epoch: epoch1.clone(),
        policy: policy.clone(),
    };
    let (status, body) = native_request(
        &fixture.service,
        "POST",
        "/diagnostics/fence/evaluate",
        Value::Null,
        serde_json::to_value(&request).expect("request serializes"),
    );
    assert_eq!(status, 200);
    let evaluated: DiagnosticGateDecisionV1 =
        serde_json::from_value(body).expect("typed decision body");
    assert_typed_verdict(&evaluated, &snapshot, &policy);
    assert_eq!(evaluated.outcome, GateOutcome::DirtyExact);
    assert_eq!(evaluated, awaited);

    let explain = fixture.lock().snapshot_explain(REPO, WORKTREE).unwrap();
    assert_eq!(explain["outcome"], json!("dirty_exact"));
    assert_eq!(
        explain["blockingIssueIds"],
        json!(awaited.blocking_issue_ids)
    );
}
