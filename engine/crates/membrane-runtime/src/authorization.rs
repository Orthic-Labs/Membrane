//! Native-path authorization gate (pending §15) — Rust port of the JS authority
//! implementation: `mcp/authorization.mjs` (`intersectAuthority`,
//! `authorizeTarget`), `mcp/project-registry.mjs` (installation registry), and
//! `mcp/server.mjs authorize` (caller/target binding verification).
//!
//! MBR-002 / SN-NODE-02 monotone effective authority: effective privilege is the
//! minimum (intersection) of the installation, caller, target, child-grant, and
//! task/session authorities. No downstream call may recover authority from the
//! target binding alone.
//!
//! Bearer transport authenticates the channel, never the scope. A self-declared
//! `repositoryId`/`scopeId` is a claim verified against the installation
//! registry — the same `project-registry.json` the JS surface enrolls into
//! (`MEMBRANE_PROJECT_REGISTRY` overrides the default
//! `APPDATA|HOME/Cortex/project-registry.json` path, mirroring
//! `defaultRegistryPath()`).
//!
//! Gate order (pending §15): installation grant → repository scope chain →
//! caller/target binding → authority level (monotone minimum) → cross-root
//! denial → validity interval/revocation. Every failure is a typed
//! [`AuthorizationDenial`] naming the failed gate — never silent scope widening,
//! never downgrade-and-continue.

use serde::Serialize;
use serde_json::Value;
use std::path::{Path, PathBuf};

/// Installation-authority ceiling (mcp/server.mjs `INSTALLATION_AUTHORITY_LEVEL`):
/// until a per-installation policy exists, the platform admission cap is
/// write-proposed so it never caps a legitimate caller below its own declared
/// level, while still bounding an over-privileged registry claim.
pub const INSTALLATION_AUTHORITY_LEVEL: AuthorityLevel = AuthorityLevel::WriteProposed;

/// Versioned request-envelope identity for the §15 gate. Bumped only when
/// [`AuthorizationRequest`] changes wire meaning.
pub const AUTHORIZATION_REQUEST_SCHEMA_VERSION: &str = "membrane.authorization-request.v1";

const RUNTIME_MANIFEST_RELATIVE: &str = "tools/lib/memory/runtime.json";
const RUNTIME_SERVICE_ID: &str = "membrane-local-v1";

/// Monotone authority levels (`mcp/authorization.mjs LEVEL_RANK`). The order is
/// the rank: a higher variant is strictly more authority.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub enum AuthorityLevel {
    #[serde(rename = "read-only")]
    ReadOnly,
    #[serde(rename = "write-proposed")]
    WriteProposed,
    #[serde(rename = "write-trusted")]
    WriteTrusted,
    #[serde(rename = "admin")]
    Admin,
}

impl AuthorityLevel {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ReadOnly => "read-only",
            Self::WriteProposed => "write-proposed",
            Self::WriteTrusted => "write-trusted",
            Self::Admin => "admin",
        }
    }

    /// Total order rank (`levelRank`). Unknown spellings never guess: the
    /// caller maps the JS `unknown_authority_level:<level>` refusal into a
    /// typed denial at the authority-level gate. Spellings compare exactly,
    /// like the JS `LEVEL_RANK` membership check.
    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            "read-only" => Ok(Self::ReadOnly),
            "write-proposed" => Ok(Self::WriteProposed),
            "write-trusted" => Ok(Self::WriteTrusted),
            "admin" => Ok(Self::Admin),
            other => Err(format!("unknown_authority_level:{other}")),
        }
    }

    pub fn rank(self) -> u8 {
        match self {
            Self::ReadOnly => 0,
            Self::WriteProposed => 1,
            Self::WriteTrusted => 2,
            Self::Admin => 3,
        }
    }
}

/// The named gates of `AuthorizationGateV1`, in execution order (§15).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum AuthorizationGate {
    InstallationGrant,
    RepositoryScopeChain,
    CallerTargetBinding,
    AuthorityLevel,
    CrossRootDenial,
    ValidityRevocation,
}

impl AuthorizationGate {
    /// Stable error code surfaced to the caller; the failed gate is always named.
    pub fn code(self) -> &'static str {
        match self {
            Self::InstallationGrant => "installation_grant_denied",
            Self::RepositoryScopeChain => "repository_scope_chain_denied",
            Self::CallerTargetBinding => "caller_scope_binding_denied",
            Self::AuthorityLevel => "caller_not_authorized",
            Self::CrossRootDenial => "cross_root_binding_denied",
            Self::ValidityRevocation => "authorization_revoked",
        }
    }
}

/// Typed authorization refusal. Never silenced, never downgraded: callers must
/// surface `code` (and the gate it names) to the wire.
#[derive(Debug, Clone, thiserror::Error)]
#[error("authorization denied at {gate:?}: {detail}")]
pub struct AuthorizationDenial {
    gate: AuthorizationGate,
    detail: String,
}

impl AuthorizationDenial {
    fn new(gate: AuthorizationGate, detail: impl Into<String>) -> Self {
        Self {
            gate,
            detail: detail.into(),
        }
    }

    pub fn gate(&self) -> AuthorizationGate {
        self.gate
    }

    pub fn code(&self) -> &'static str {
        self.gate.code()
    }

    pub fn detail(&self) -> &str {
        &self.detail
    }
}

/// One enrolled repository binding — the Rust mirror of a JS project-registry
/// entry (`mcp/project-registry.mjs publicBinding`).
#[derive(Debug, Clone, Serialize)]
pub struct RepositoryBindingV1 {
    pub root: String,
    pub repository_id: String,
    pub scope_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scope_descriptor: Option<Value>,
    pub child_repository_ids: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub grant_level: Option<AuthorityLevel>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token_generation: Option<u64>,
    pub revoked_token_generations: Vec<u64>,
}

/// The installation registry: every repository binding enrolled on this
/// installation. Loading is fail-closed — a malformed registry is an
/// installation-grant denial, never an empty allow-all.
#[derive(Debug, Clone, Default)]
pub struct InstallationRegistryV1 {
    bindings: Vec<RepositoryBindingV1>,
}

impl InstallationRegistryV1 {
    pub fn bindings(&self) -> &[RepositoryBindingV1] {
        &self.bindings
    }

    fn binding_for_root(&self, root: &str) -> Option<&RepositoryBindingV1> {
        self.bindings
            .iter()
            .find(|binding| binding.root == root || paths_equal_loose(&binding.root, root))
    }

    fn binding_for_repository(&self, repository_id: &str) -> Option<&RepositoryBindingV1> {
        self.bindings
            .iter()
            .find(|binding| binding.repository_id == repository_id)
    }
}

/// Root-key comparison with the same effective tolerance the JS registry has
/// after `realpath`: the Windows canonical-device prefix is inert, and Windows
/// paths compare case-insensitively.
fn paths_equal_loose(left: &str, right: &str) -> bool {
    let strip = |value: &str| {
        value
            .strip_prefix(r"\\?\")
            .unwrap_or(value)
            .trim_end_matches(['\\', '/'])
            .to_owned()
    };
    let (left, right) = (strip(left), strip(right));
    if cfg!(windows) {
        left.eq_ignore_ascii_case(&right)
    } else {
        left == right
    }
}

/// Registry path resolution mirroring `mcp/project-registry.mjs
/// defaultRegistryPath`: `MEMBRANE_PROJECT_REGISTRY` overrides, else
/// `APPDATA` (Windows) or `HOME` + `Cortex/project-registry.json`.
pub fn default_registry_path() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("MEMBRANE_PROJECT_REGISTRY") {
        return Some(PathBuf::from(path));
    }
    let base = std::env::var_os("APPDATA").or_else(|| std::env::var_os("HOME"))?;
    Some(
        PathBuf::from(base)
            .join("Cortex")
            .join("project-registry.json"),
    )
}

/// Live filesystem re-derivation, never a trusted stored snapshot: the registry
/// is read on every authorization so a rotated or revoked binding takes effect
/// immediately (mcp/server.mjs `authorize` reads `bindingFor` per request).
///
/// A missing registry file is the JS `ENOENT → empty bindings` case; a
/// malformed one fails closed as an installation-grant denial.
pub fn load_installation_registry() -> Result<InstallationRegistryV1, AuthorizationDenial> {
    let Some(path) = default_registry_path() else {
        return Err(AuthorizationDenial::new(
            AuthorizationGate::InstallationGrant,
            "installation registry path is unresolvable: no APPDATA/HOME base",
        ));
    };
    let raw = match std::fs::read(&path) {
        Ok(raw) => raw,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(InstallationRegistryV1::default());
        }
        Err(error) => {
            return Err(AuthorizationDenial::new(
                AuthorizationGate::InstallationGrant,
                format!("registry unavailable {}: {error}", path.display()),
            ));
        }
    };
    let value: Value = serde_json::from_slice(&raw).map_err(|error| {
        AuthorizationDenial::new(
            AuthorizationGate::InstallationGrant,
            format!("registry unavailable {}: {error}", path.display()),
        )
    })?;
    parse_registry(&value, &path)
}

fn parse_registry(
    value: &Value,
    path: &Path,
) -> Result<InstallationRegistryV1, AuthorizationDenial> {
    let deny = |detail: String| {
        AuthorizationDenial::new(
            AuthorizationGate::InstallationGrant,
            format!("registry {} is malformed: {detail}", path.display()),
        )
    };
    let schema_version = value.get("schema_version").and_then(Value::as_u64);
    if !matches!(schema_version, Some(1) | Some(2)) {
        return Err(deny("schema_version must be 1 or 2".into()));
    }
    let Some(bindings) = value.get("bindings").and_then(Value::as_object) else {
        return Err(deny("bindings must be an object".into()));
    };
    let mut parsed = Vec::with_capacity(bindings.len());
    for (root, binding) in bindings {
        if root.trim().is_empty() {
            return Err(deny("registry root is malformed".into()));
        }
        parsed.push(parse_binding(root, binding).map_err(deny)?);
    }
    Ok(InstallationRegistryV1 { bindings: parsed })
}

fn parse_binding(root: &str, binding: &Value) -> Result<RepositoryBindingV1, String> {
    let object = binding
        .as_object()
        .ok_or_else(|| "binding must be an object".to_string())?;
    let mut repository_id = None;
    let mut scope_id = None;
    for key in ["repository_id", "scope_id"] {
        let value = object
            .get(key)
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| format!("binding {key} is required"))?;
        if key == "repository_id" {
            repository_id = Some(value.to_owned());
        } else {
            scope_id = Some(value.to_owned());
        }
    }
    let scope_descriptor = object
        .get("scope_descriptor")
        .cloned()
        .filter(Value::is_object);
    let grant_policy = object.get("grant_policy");
    let grant_level = match grant_policy.and_then(|policy| policy.get("level")) {
        None | Some(Value::Null) => None,
        Some(Value::String(level)) => Some(
            AuthorityLevel::parse(level)
                .map_err(|_| format!("grant_policy.level is invalid: {level}"))?,
        ),
        Some(_) => return Err("grant_policy.level must be a string".into()),
    };
    let child_repository_ids =
        match grant_policy.and_then(|policy| policy.get("child_repository_ids")) {
            None | Some(Value::Null) => Vec::new(),
            Some(Value::Array(values)) => values
                .iter()
                .map(|value| {
                    value
                        .as_str()
                        .map(str::to_owned)
                        .ok_or_else(|| "child_repository_ids must contain strings".to_string())
                })
                .collect::<Result<Vec<_>, _>>()?,
            Some(_) => return Err("child_repository_ids must be an array".into()),
        };
    let token_grant = object.get("token_grant").filter(|grant| grant.is_object());
    let token_generation = match token_grant.and_then(|grant| grant.get("generation")) {
        None => None,
        Some(Value::Number(generation)) => {
            let generation = generation
                .as_u64()
                .filter(|generation| *generation >= 1)
                .ok_or_else(|| "token_grant generation is invalid".to_string())?;
            Some(generation)
        }
        Some(_) => return Err("token_grant generation is invalid".into()),
    };
    let revoked_token_generations =
        match token_grant.and_then(|grant| grant.get("revoked_generations")) {
            None | Some(Value::Null) => Vec::new(),
            Some(Value::Array(values)) => values
                .iter()
                .map(|value| {
                    value
                        .as_u64()
                        .filter(|generation| *generation >= 1)
                        .ok_or_else(|| "token_grant revocations are invalid".to_string())
                })
                .collect::<Result<Vec<_>, _>>()?,
            Some(_) => return Err("token_grant revocations are invalid".into()),
        };
    if let Some(generation) = token_generation {
        if revoked_token_generations
            .iter()
            .any(|revoked| *revoked >= generation)
        {
            return Err("token_grant revocations are invalid".into());
        }
    }
    if let Some(issued_at) = token_grant.and_then(|grant| grant.get("issued_at")) {
        if issued_at.as_str().unwrap_or("").trim().is_empty() {
            return Err("token_grant issued_at is required".into());
        }
    }
    Ok(RepositoryBindingV1 {
        root: root.to_owned(),
        repository_id: repository_id.expect("validated above"),
        scope_id: scope_id.expect("validated above"),
        scope_descriptor,
        child_repository_ids,
        grant_level,
        token_generation,
        revoked_token_generations,
    })
}

/// Stable-key canonical JSON comparison of scope descriptors
/// (mcp/server.mjs `stableDescriptor`/`sameDescriptor`): key order never
/// changes identity.
fn same_descriptor(left: &Value, right: &Value) -> bool {
    fn stable(value: &Value) -> String {
        match value {
            Value::Array(values) => {
                let inner: Vec<String> = values.iter().map(stable).collect();
                format!("[{}]", inner.join(","))
            }
            Value::Object(map) => {
                let mut keys: Vec<&String> = map.keys().collect();
                keys.sort();
                // `format!("{key:?}")` is the infallible quoted-key form; the
                // comparison only needs a stable bijection, not valid JSON.
                let inner: Vec<String> = keys
                    .into_iter()
                    .map(|key| format!("{key:?}:{}", stable(&map[key])))
                    .collect();
                format!("{{{}}}", inner.join(","))
            }
            other => other.to_string(),
        }
    }
    stable(left) == stable(right)
}

fn caller_descriptor(scope_id: &str, declared: Option<&Value>) -> Value {
    declared
        .cloned()
        .unwrap_or_else(|| serde_json::json!({"kind": "filesystem", "path": scope_id}))
}

fn binding_descriptor(binding: &RepositoryBindingV1) -> Value {
    binding
        .scope_descriptor
        .clone()
        .unwrap_or_else(|| serde_json::json!({"kind": "filesystem", "path": binding.scope_id}))
}

/// Everything the gate needs for one repository-scoped native request.
#[derive(Debug, Clone)]
pub struct AuthorizationRequest<'a> {
    pub caller_root: &'a str,
    pub caller_repository_id: &'a str,
    pub caller_scope_id: &'a str,
    pub caller_scope_descriptor: Option<&'a Value>,
    /// Declared target repository. On the native single-repo path this is the
    /// caller's own `repositoryId`; a distinct value reaches a granted child.
    pub target_repository: &'a str,
    /// Explicit task/session authority carried on the envelope. Absent means
    /// the caller's own persisted level is the task authority
    /// (`mcp/server.mjs effectiveAuthorityFor`, lines 192-201, used by the
    /// direct `authorize` path at line ~250) so an absent grant never
    /// cold-caps legitimate same-root work below the caller's persisted
    /// level. The workspace fan-out primitive [`can_reach_target`] applies a
    /// different, explicit read-only clamp — see its doc comment for the
    /// `mcp/authorization.mjs` source it mirrors.
    pub task_grant_level: Option<&'a str>,
    /// Operation action name (`context`, `source_read`, `checkpoint`, …) using
    /// the JS surface's action vocabulary.
    pub action: &'a str,
}

/// The authorized outcome of one gate pass.
#[derive(Debug, Clone, Serialize)]
pub struct AuthorizationDecisionV1 {
    pub effective_level: AuthorityLevel,
    pub caller_repository_id: String,
    pub target_repository_id: String,
    pub same_root: bool,
    pub granted_child: bool,
}

/// Read actions admit at `read-only`; every mutating action requires at least
/// `write-proposed` (`mcp/authorization.mjs READ_ACTIONS` + `permitsLevel`).
pub fn is_read_action(action: &str) -> bool {
    matches!(
        action,
        "context"
            | "source_read"
            | "checkpoint_load"
            | "working_context_load"
            | "temporal_fact_query"
            | "scratchpad_load"
            | "system_status"
    )
}

/// Monotone minimum of the provided authorities. Absent (None) slots are
/// skipped; an all-absent intersection is the fail-closed `read-only`
/// (`mcp/authorization.mjs intersectAuthority`).
pub fn intersect_authority(levels: &[Option<AuthorityLevel>]) -> AuthorityLevel {
    levels
        .iter()
        .flatten()
        .min()
        .copied()
        .unwrap_or(AuthorityLevel::ReadOnly)
}

pub fn permits_level(level: AuthorityLevel, action: &str) -> bool {
    if is_read_action(action) {
        level.rank() >= AuthorityLevel::ReadOnly.rank()
    } else {
        level.rank() >= AuthorityLevel::WriteProposed.rank()
    }
}

/// Run the full `AuthorizationGateV1` sequence for one request.
pub fn authorize(
    request: &AuthorizationRequest<'_>,
) -> Result<AuthorizationDecisionV1, AuthorizationDenial> {
    // Gate 1 — installation grant: the installation registry must load and
    // validate; its (write-proposed) authority ceiling participates below.
    let registry = load_installation_registry()?;
    let installation_level = Some(INSTALLATION_AUTHORITY_LEVEL);

    // Gate 2 — repository scope chain: caller root and target repository must
    // both resolve to enrolled bindings on this installation.
    let caller_binding = registry
        .binding_for_root(request.caller_root)
        .ok_or_else(|| {
            AuthorizationDenial::new(
                AuthorizationGate::RepositoryScopeChain,
                format!(
                    "caller root {} is not enrolled in the installation registry",
                    request.caller_root
                ),
            )
        })?;
    let target_binding = if registry
        .binding_for_root(request.target_repository)
        .is_some_and(|binding| paths_equal_loose(&binding.root, &caller_binding.root))
    {
        // The target was declared as the caller's own root path.
        caller_binding
    } else if request.target_repository == caller_binding.repository_id {
        caller_binding
    } else {
        registry
            .binding_for_repository(request.target_repository)
            .ok_or_else(|| {
                AuthorizationDenial::new(
                    AuthorizationGate::RepositoryScopeChain,
                    format!(
                        "target repository {} is not enrolled in the installation registry",
                        request.target_repository
                    ),
                )
            })?
    };
    let same_root = caller_binding.root == target_binding.root;
    let granted_child = !same_root && has_explicit_child_grant(caller_binding, target_binding);

    // Gate 3 — caller/target binding: the caller must accurately self-report
    // ITS OWN identity, checked against the caller's persisted registry entry,
    // not the target's.
    let declared_descriptor =
        caller_descriptor(request.caller_scope_id, request.caller_scope_descriptor);
    let identity_matches = request.caller_repository_id == caller_binding.repository_id
        && request.caller_scope_id == caller_binding.scope_id
        && same_descriptor(&declared_descriptor, &binding_descriptor(caller_binding));
    if !identity_matches {
        return Err(AuthorizationDenial::new(
            AuthorizationGate::CallerTargetBinding,
            format!(
                "caller identity ({}, {}) does not match the installation registry binding for {}",
                request.caller_repository_id, request.caller_scope_id, request.caller_root
            ),
        ));
    }

    // Gate 4 — authority level (monotone minimum): installation × caller ×
    // target × child-grant × task/session. An unknown level spelling is a
    // denial, never a guess.
    //
    // Gate order (pending doc §15): authority level runs BEFORE cross-root
    // denial (Gate 5), deliberately diverging from mcp/authorization.mjs
    // `authorizeTarget`, which checks cross-root first. The divergence changes
    // only which gate name a denial reports — a request that is both
    // cross-root-ungranted and under-privileged denies at AuthorityLevel here
    // where the JS surface reports cross_root_binding_denied. Both orders
    // deny; neither widens access.
    let parse_level = |value: Option<&str>, source: &str| {
        value
            .map(AuthorityLevel::parse)
            .transpose()
            .map_err(|detail| {
                AuthorizationDenial::new(
                    AuthorizationGate::AuthorityLevel,
                    format!("{source}: {detail}"),
                )
            })
    };
    // Registry-sourced levels were validated at load; only the task grant
    // arrives on the wire and needs its spelling checked here.
    let caller_level = caller_binding
        .grant_level
        .unwrap_or(AuthorityLevel::ReadOnly);
    let target_level = target_binding
        .grant_level
        .unwrap_or(AuthorityLevel::ReadOnly);
    let task_grant = parse_level(request.task_grant_level, "taskGrantLevel")?;
    let child_grant_slot = if same_root {
        Some(AuthorityLevel::Admin)
    } else if granted_child {
        Some(target_level)
    } else {
        Some(AuthorityLevel::ReadOnly)
    };
    let effective_level = intersect_authority(&[
        installation_level,
        Some(caller_level),
        Some(target_level),
        child_grant_slot,
        // DIRECT-path task authority (mcp/server.mjs effectiveAuthorityFor,
        // lines 192-201): an absent envelope grant falls back to the caller's
        // own persisted level so legitimate same-root work is never cold-capped
        // below it. This is deliberately NOT the fan-out default — the
        // workspace fan-out clamps to read-only instead (see
        // can_reach_target). Falling back to caller_level is safe as a default
        // because the caller level itself is already an intersection input.
        task_grant.or(Some(caller_level)),
    ]);
    if !permits_level(effective_level, request.action) {
        return Err(AuthorizationDenial::new(
            AuthorizationGate::AuthorityLevel,
            format!(
                "effective authority {} may not perform {} on {}",
                effective_level.as_str(),
                request.action,
                target_binding.repository_id
            ),
        ));
    }

    // Gate 5 — cross-root denial: a distinct-root call is reachable only when
    // the caller's persisted grant names the target AND the live filesystem
    // agrees the target is a child of that workspace (the catalog-agreement
    // re-derivation of mcp/repository-catalog.mjs hasExplicitChildGrant).
    if !same_root && !granted_child {
        return Err(AuthorizationDenial::new(
            AuthorizationGate::CrossRootDenial,
            format!(
                "caller root {} has no explicit child grant for {}",
                caller_binding.root, target_binding.root
            ),
        ));
    }

    // Gate 6 — validity interval/revocation: a revoked token generation is a
    // dead grant; the current generation must be live.
    if let Some(generation) = target_binding.token_generation {
        if target_binding
            .revoked_token_generations
            .contains(&generation)
        {
            return Err(AuthorizationDenial::new(
                AuthorizationGate::ValidityRevocation,
                format!("target binding token generation {generation} is revoked"),
            ));
        }
    }
    if let Some(generation) = caller_binding.token_generation {
        if caller_binding
            .revoked_token_generations
            .contains(&generation)
        {
            return Err(AuthorizationDenial::new(
                AuthorizationGate::ValidityRevocation,
                format!("caller binding token generation {generation} is revoked"),
            ));
        }
    }

    Ok(AuthorizationDecisionV1 {
        effective_level,
        caller_repository_id: caller_binding.repository_id.clone(),
        target_repository_id: target_binding.repository_id.clone(),
        same_root,
        granted_child,
    })
}

/// The workspace aggregate must exercise exactly the same per-target
/// authorization primitive as a direct repository call (MBR-003). Returns the
/// effective level when the caller may reach the target for `action`; `None`
/// on any denial — the fan-out renders that target as a typed omission row,
/// it never silently widens scope.
///
/// Explicit fan-out task-authority contract: this primitive mirrors
/// `mcp/authorization.mjs canReachTarget` → `authorizeTarget` (line 52's
/// `taskGrantLevel || "read-only"`), NOT the direct-path caller-level fallback
/// of `mcp/server.mjs effectiveAuthorityFor`. The clamp is applied explicitly
/// below because this helper funnels through [`authorize`], whose absent-grant
/// default is the caller's own level. A mutating action therefore returns
/// `None` unconditionally here (fail closed); a caller carrying a real
/// task/session grant must call [`authorize`] directly.
pub fn can_reach_target(
    caller_root: &str,
    caller_repository_id: &str,
    caller_scope_id: &str,
    target_repository: &str,
    action: &str,
) -> Option<AuthorityLevel> {
    authorize(&AuthorizationRequest {
        caller_root,
        caller_repository_id,
        caller_scope_id,
        caller_scope_descriptor: None,
        target_repository,
        // FAN-OUT task clamp (mcp/authorization.mjs authorizeTarget, line 52:
        // `taskGrantLevel || "read-only"`): applied explicitly because the
        // direct path this funnels through now falls back to caller_level on
        // an absent grant. The read-only slot cannot widen: it can only clamp
        // the intersection, so mutations fail closed (None) on this path.
        task_grant_level: Some("read-only"),
        action,
    })
    .ok()
    .map(|decision| decision.effective_level)
}

/// Port of mcp/repository-catalog.mjs `hasExplicitChildGrant` + the live
/// catalog agreement: the caller's persisted `child_repository_ids` grant must
/// name the target and the target must actually be a filesystem child of the
/// caller's workspace root, re-derived from disk on every check. Fails closed
/// whenever the caller carries no explicit grant or the containment cannot be
/// proven.
fn has_explicit_child_grant(caller: &RepositoryBindingV1, target: &RepositoryBindingV1) -> bool {
    if !caller
        .child_repository_ids
        .iter()
        .any(|granted| granted == &target.repository_id)
    {
        return false;
    }
    let caller_root = Path::new(&caller.root);
    let target_root = Path::new(&target.root);
    match (caller_root.canonicalize(), target_root.canonicalize()) {
        (Ok(caller_root), Ok(target_root)) => target_root.starts_with(&caller_root),
        _ => false,
    }
}

/// The installation binding the JS surface resolves for a caller root
/// (`mcp/installation-binding.mjs` semantics): the workspace runtime manifest
/// must exist above the root, name the membrane-local service, bind loopback,
/// and carry a valid port. Used by the native path to bind the installation
/// identity the grant chain trusts.
pub fn installation_binding_root_for(caller_root: &str) -> Result<PathBuf, AuthorizationDenial> {
    let mut current = PathBuf::from(caller_root);
    loop {
        if current.join(RUNTIME_MANIFEST_RELATIVE).is_file() {
            return Ok(current);
        }
        if !current.pop() {
            break;
        }
    }
    Err(AuthorizationDenial::new(
        AuthorizationGate::InstallationGrant,
        format!(
            "installation binding unavailable: {RUNTIME_MANIFEST_RELATIVE} not found above {caller_root}"
        ),
    ))
}

/// Validate the workspace runtime manifest found at a binding root
/// (`mcp/installation-binding.mjs readRuntime`): valid loopback port,
/// expected serviceId, loopback host.
pub fn validate_installation_manifest(binding_root: &Path) -> Result<(), AuthorizationDenial> {
    let manifest_path = binding_root.join(RUNTIME_MANIFEST_RELATIVE);
    let raw = std::fs::read(&manifest_path).map_err(|error| {
        AuthorizationDenial::new(
            AuthorizationGate::InstallationGrant,
            format!(
                "installation binding unavailable {}: {error}",
                manifest_path.display()
            ),
        )
    })?;
    let manifest: Value = serde_json::from_slice(&raw).map_err(|error| {
        AuthorizationDenial::new(
            AuthorizationGate::InstallationGrant,
            format!(
                "installation binding unavailable {}: {error}",
                manifest_path.display()
            ),
        )
    })?;
    let deny = |detail: String| {
        AuthorizationDenial::new(
            AuthorizationGate::InstallationGrant,
            format!("installation binding unavailable: {detail}"),
        )
    };
    let port = manifest
        .get("port")
        .and_then(Value::as_u64)
        .ok_or_else(|| deny("runtime port is invalid".into()))?;
    if !(1024..=65535).contains(&port) {
        return Err(deny("runtime port is invalid".into()));
    }
    if manifest.get("serviceId").and_then(Value::as_str) != Some(RUNTIME_SERVICE_ID) {
        return Err(deny("runtime serviceId mismatch".into()));
    }
    if let Some(host) = manifest.get("host").and_then(Value::as_str) {
        if !host.is_empty() && host != "127.0.0.1" {
            return Err(deny("runtime host must be loopback".into()));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn level(value: &str) -> AuthorityLevel {
        AuthorityLevel::parse(value).expect("test level is valid")
    }

    #[test]
    fn intersect_authority_returns_the_monotone_minimum() {
        assert_eq!(
            intersect_authority(&[Some(level("read-only")), Some(level("write-trusted"))]),
            AuthorityLevel::ReadOnly
        );
        assert_eq!(
            intersect_authority(&[Some(level("write-proposed")), Some(level("write-trusted"))]),
            AuthorityLevel::WriteProposed
        );
        assert_eq!(
            intersect_authority(&[Some(level("write-trusted")), Some(level("admin"))]),
            AuthorityLevel::WriteTrusted
        );
        assert_eq!(
            intersect_authority(&[Some(level("admin"))]),
            AuthorityLevel::Admin
        );
        assert_eq!(intersect_authority(&[]), AuthorityLevel::ReadOnly);
        assert_eq!(intersect_authority(&[None, None]), AuthorityLevel::ReadOnly);
    }

    #[test]
    fn intersect_authority_rejects_an_unknown_level_instead_of_guessing() {
        assert_eq!(
            AuthorityLevel::parse("superuser").unwrap_err(),
            "unknown_authority_level:superuser"
        );
    }

    #[test]
    fn permits_level_admits_reads_at_read_only_and_mutations_at_write_proposed_plus() {
        for action in ["context", "source_read", "checkpoint_load", "system_status"] {
            assert!(permits_level(AuthorityLevel::ReadOnly, action), "{action}");
        }
        for action in ["feedback", "checkpoint", "proposal"] {
            assert!(!permits_level(AuthorityLevel::ReadOnly, action), "{action}");
            assert!(
                permits_level(AuthorityLevel::WriteProposed, action),
                "{action}"
            );
            assert!(
                permits_level(AuthorityLevel::WriteTrusted, action),
                "{action}"
            );
        }
    }

    #[test]
    fn level_rank_exposes_the_total_order() {
        assert!(AuthorityLevel::ReadOnly < AuthorityLevel::WriteProposed);
        assert!(AuthorityLevel::WriteProposed < AuthorityLevel::WriteTrusted);
        assert!(AuthorityLevel::WriteTrusted < AuthorityLevel::Admin);
    }

    #[test]
    fn same_descriptor_ignores_key_order_and_defaults_to_filesystem_scope() {
        assert!(same_descriptor(
            &json!({"kind": "filesystem", "path": "D--Claude"}),
            &json!({"path": "D--Claude", "kind": "filesystem"})
        ));
        assert!(same_descriptor(
            &caller_descriptor("D--Claude", None),
            &json!({"kind": "filesystem", "path": "D--Claude"})
        ));
        assert!(!same_descriptor(
            &json!({"kind": "virtual", "id": "a", "tenant_id": "t", "parents": ["p"], "inherit_global": false}),
            &json!({"kind": "virtual", "id": "a", "tenant_id": "t", "parents": [], "inherit_global": false})
        ));
    }

    #[test]
    fn registry_parsing_rejects_malformed_grants_fail_closed() {
        let binding = json!({
            "repository_id": "repo-a",
            "scope_id": "D--Claude-a",
            "grant_policy": {"level": "nonsense"}
        });
        assert!(parse_binding("D:\\Claude\\a", &binding)
            .unwrap_err()
            .contains("grant_policy.level is invalid"));

        let binding = json!({
            "repository_id": "repo-a",
            "scope_id": "D--Claude-a",
            "token_grant": {"generation": 2, "revoked_generations": [3]}
        });
        assert_eq!(
            parse_binding("D:\\Claude\\a", &binding).unwrap_err(),
            "token_grant revocations are invalid"
        );

        let binding = json!({"repository_id": "", "scope_id": "D--Claude-a"});
        assert_eq!(
            parse_binding("D:\\Claude\\a", &binding).unwrap_err(),
            "binding repository_id is required"
        );
    }

    #[test]
    fn installation_manifest_validation_is_fail_closed() {
        let root = tempfile::tempdir().expect("temp root");
        let manifest = root.path().join("tools/lib/memory/runtime.json");
        std::fs::create_dir_all(manifest.parent().unwrap()).expect("mkdir");
        std::fs::write(
            &manifest,
            json!({"port": 47851, "serviceId": "membrane-local-v1", "host": "127.0.0.1"})
                .to_string(),
        )
        .expect("write manifest");
        assert!(validate_installation_manifest(root.path()).is_ok());
        let resolved = installation_binding_root_for(root.path().to_str().unwrap())
            .expect("binding root resolves");
        assert!(paths_equal_loose(
            resolved.to_str().expect("utf8 path"),
            root.path().to_str().expect("utf8 path")
        ));

        std::fs::write(
            &manifest,
            json!({"port": 47851, "serviceId": "other-service", "host": "127.0.0.1"}).to_string(),
        )
        .expect("rewrite manifest");
        assert_eq!(
            validate_installation_manifest(root.path())
                .unwrap_err()
                .code(),
            "installation_grant_denied"
        );

        std::fs::write(
            &manifest,
            json!({"port": 80, "serviceId": "membrane-local-v1"}).to_string(),
        )
        .expect("rewrite manifest");
        assert_eq!(
            validate_installation_manifest(root.path())
                .unwrap_err()
                .code(),
            "installation_grant_denied"
        );
    }
}
