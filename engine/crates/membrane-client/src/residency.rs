//! Public additive holder lifecycle for one canonical installed controller.

use std::collections::BTreeMap;
use std::sync::Arc;
use serde_json::{Map, Value};
use thiserror::Error;

use crate::{CallOptions, ClientError};
use membrane_protocol::{
    ResidentControllerIdentityV1, ResidentHolderCredentialV1, ResidentHolderLossV1,
    ResidentHolderOperationV1, ResidentHolderRequestV1, ResidentHolderResponseV1,
    RESIDENT_HOLDER_SCHEMA_VERSION,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum HolderKind {
    Hub,
    CodeRightDaemon,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct AuthenticatedHolder {
    pub kind: HolderKind,
    pub holder_id: String,
    pub credential_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ControllerIdentity {
    pub installation_id: String,
    pub cortex_store_id: String,
    pub release_generation: String,
    pub startup_generation: u64,
}

/// Resident identity is intentionally distinct from `ExplicitOwnerBindingV1`:
/// explicit calls remain bounded and never imply a background holder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResidentControllerBinding {
    canonical: crate::CanonicalBinding,
}

impl ResidentControllerBinding {
    /// Construct resident identity only from a binding already verified by
    /// `bind_candidate`; callers cannot supply or refresh identity fields.
    pub fn from_canonical(canonical: crate::CanonicalBinding) -> Result<Self, ClientError> {
        if canonical.identity.runtime_origin != "installed"
            || canonical.identity.stable_install_root.as_deref().is_none_or(|root|
                crate::binding::comparable_stable_root(root)
                    != crate::binding::comparable_stable_root(&canonical.candidate.stable_install_root))
        {
            return Err(ClientError::Incompatible {
                message: "resident controller requires verified installed canonical binding".into(),
            });
        }
        Ok(Self { canonical })
    }

    pub fn canonical(&self) -> &crate::CanonicalBinding {
        &self.canonical
    }

    pub fn protocol_identity(&self) -> Result<ResidentControllerIdentityV1, ClientError> {
        let identity = &self.canonical.identity;
        let controller = ControllerIdentity {
            installation_id: identity.installation_id.clone(),
            cortex_store_id: identity.cortex_store_id.clone(),
            release_generation: identity.release_generation.clone(),
            startup_generation: identity.startup_generation,
        };
        validate_controller(&controller).map_err(|error| ClientError::InvalidRequest {
            message: error.to_string(),
        })?;
        Ok(ResidentControllerIdentityV1 {
            installation_id: identity.installation_id.clone(),
            cortex_store_id: identity.cortex_store_id.clone(),
            release_generation: identity.release_generation.clone(),
            startup_generation: identity.startup_generation,
            stable_current: self.canonical.candidate.stable_install_root.clone(),
        })
    }
}

/// Injected installed-controller transport.  SDK code owns request/response
/// fences; hosts choose their authenticated local IPC implementation.
pub trait ResidentHolderTransport: Send + Sync {
    fn call(&self, request: ResidentHolderRequestV1) -> Result<ResidentHolderResponseV1, ClientError>;
}

impl<F> ResidentHolderTransport for F
where
    F: Fn(ResidentHolderRequestV1) -> Result<ResidentHolderResponseV1, ClientError> + Send + Sync,
{
    fn call(&self, request: ResidentHolderRequestV1) -> Result<ResidentHolderResponseV1, ClientError> {
        self(request)
    }
}

pub struct InstalledResidentController<T: ResidentHolderTransport + ?Sized = dyn ResidentHolderTransport> {
    binding: ResidentControllerBinding,
    transport: Arc<T>,
    options: CallOptions,
}

pub struct ResidentHolderCall<'a, T: ResidentHolderTransport + ?Sized> {
    controller: &'a InstalledResidentController<T>,
    options: CallOptions,
}

impl<T: ResidentHolderTransport + ?Sized> InstalledResidentController<T> {
    pub fn new(binding: ResidentControllerBinding, transport: Arc<T>) -> Result<Self, ClientError> {
        binding.protocol_identity()?;
        Ok(Self { binding, transport, options: CallOptions::after(std::time::Duration::from_secs(30)) })
    }

    pub fn binding(&self) -> &ResidentControllerBinding {
        &self.binding
    }

    pub fn with_call_options(&self, options: CallOptions) -> ResidentHolderCall<'_, T> {
        ResidentHolderCall { controller: self, options }
    }

    pub fn acquire(
        &self,
        holder: AuthenticatedHolder,
        observed_at_unix_ms: u64,
        expires_at_unix_ms: u64,
    ) -> Result<ResidentHolderResponseV1, ClientError> {
        self.call(ResidentHolderOperationV1::Acquire, Some(holder), observed_at_unix_ms, Some(expires_at_unix_ms), None)
    }

    pub fn renew(
        &self,
        holder: AuthenticatedHolder,
        observed_at_unix_ms: u64,
        expires_at_unix_ms: u64,
    ) -> Result<ResidentHolderResponseV1, ClientError> {
        self.call(ResidentHolderOperationV1::Renew, Some(holder), observed_at_unix_ms, Some(expires_at_unix_ms), None)
    }

    pub fn release(
        &self,
        holder: AuthenticatedHolder,
        observed_at_unix_ms: u64,
    ) -> Result<ResidentHolderResponseV1, ClientError> {
        self.call(ResidentHolderOperationV1::Release, Some(holder), observed_at_unix_ms, None, None)
    }

    pub fn status(&self, observed_at_unix_ms: u64) -> Result<ResidentHolderResponseV1, ClientError> {
        self.call(ResidentHolderOperationV1::Status, None, observed_at_unix_ms, None, None)
    }

    pub fn subscribe_loss(
        &self,
        observed_at_unix_ms: u64,
        loss_cursor: u64,
    ) -> Result<Option<ResidentHolderLossV1>, ClientError> {
        Ok(self
            .call(ResidentHolderOperationV1::SubscribeLoss, None, observed_at_unix_ms, None, Some(loss_cursor))?
            .loss)
    }

    fn call(
        &self,
        operation: ResidentHolderOperationV1,
        holder: Option<AuthenticatedHolder>,
        observed_at_unix_ms: u64,
        expires_at_unix_ms: Option<u64>,
        loss_cursor: Option<u64>,
    ) -> Result<ResidentHolderResponseV1, ClientError> {
        self.call_with_options(
            operation,
            holder,
            observed_at_unix_ms,
            expires_at_unix_ms,
            loss_cursor,
            &self.options,
        )
    }

    fn call_with_options(
        &self,
        operation: ResidentHolderOperationV1,
        holder: Option<AuthenticatedHolder>,
        observed_at_unix_ms: u64,
        expires_at_unix_ms: Option<u64>,
        loss_cursor: Option<u64>,
        options: &CallOptions,
    ) -> Result<ResidentHolderResponseV1, ClientError> {
        options.check()?;
        let controller = self.binding.protocol_identity()?;
        if matches!(operation, ResidentHolderOperationV1::Acquire | ResidentHolderOperationV1::Renew)
            && (expires_at_unix_ms.is_none()
                || expires_at_unix_ms.is_some_and(|expiry| expiry <= observed_at_unix_ms))
        {
            return Err(ClientError::InvalidRequest { message: "resident holder expiry must be after observed time".into() });
        }
        let request = ResidentHolderRequestV1 {
            schema_version: RESIDENT_HOLDER_SCHEMA_VERSION,
            operation,
            controller: controller.clone(),
            holder: holder.map(protocol_holder).transpose()?,
            expires_at_unix_ms,
            observed_at_unix_ms,
            loss_cursor,
        };
        let response = self.transport.call(request)?;
        options.check()?;
        if response.schema_version != RESIDENT_HOLDER_SCHEMA_VERSION
            || response.operation != operation
            || response.controller != controller
        {
            return Err(ClientError::Incompatible { message: "resident holder response failed installed identity fence".into() });
        }
        if operation == ResidentHolderOperationV1::SubscribeLoss {
            if response.loss.as_ref().is_some_and(|loss| loss.controller != controller) {
                return Err(ClientError::Incompatible { message: "resident holder loss failed installed identity fence".into() });
            }
        } else if response.loss.is_some() {
            return Err(ClientError::Incompatible { message: "resident holder response has incoherent loss payload".into() });
        }
        Ok(response)
    }
}

impl<'a, T: ResidentHolderTransport + ?Sized> ResidentHolderCall<'a, T> {
    pub fn acquire(&self, holder: AuthenticatedHolder, now: u64, expires_at: u64) -> Result<ResidentHolderResponseV1, ClientError> {
        self.options.check()?;
        self.controller.call_with_options(ResidentHolderOperationV1::Acquire, Some(holder), now, Some(expires_at), None, &self.options)
    }

    pub fn renew(&self, holder: AuthenticatedHolder, now: u64, expires_at: u64) -> Result<ResidentHolderResponseV1, ClientError> {
        self.options.check()?;
        self.controller.call_with_options(ResidentHolderOperationV1::Renew, Some(holder), now, Some(expires_at), None, &self.options)
    }

    pub fn release(&self, holder: AuthenticatedHolder, now: u64) -> Result<ResidentHolderResponseV1, ClientError> {
        self.options.check()?;
        self.controller.call_with_options(ResidentHolderOperationV1::Release, Some(holder), now, None, None, &self.options)
    }

    pub fn status(&self, now: u64) -> Result<ResidentHolderResponseV1, ClientError> {
        self.options.check()?;
        self.controller.call_with_options(ResidentHolderOperationV1::Status, None, now, None, None, &self.options)
    }

    pub fn subscribe_loss(&self, now: u64, cursor: u64) -> Result<Option<ResidentHolderLossV1>, ClientError> {
        self.options.check()?;
        Ok(self
            .controller
            .call_with_options(ResidentHolderOperationV1::SubscribeLoss, None, now, None, Some(cursor), &self.options)?
            .loss)
    }
}

fn protocol_holder(holder: AuthenticatedHolder) -> Result<ResidentHolderCredentialV1, ClientError> {
    validate_holder(&holder).map_err(|error| ClientError::InvalidRequest { message: error.to_string() })?;
    Ok(ResidentHolderCredentialV1 {
        holder_kind: match holder.kind {
            HolderKind::Hub => "hub",
            HolderKind::CodeRightDaemon => "coderight_daemon",
        }
        .into(),
        holder_id: holder.holder_id,
        credential_id: holder.credential_id,
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct HolderLease {
    credential_id: String,
    expires_at_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResidencySnapshot {
    pub controller: Option<ControllerIdentity>,
    pub hub_holders: usize,
    pub coderight_daemon_holders: usize,
}

impl ResidencySnapshot {
    pub fn residents_required(&self) -> bool {
        self.hub_holders + self.coderight_daemon_holders > 0
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AcquireDecision {
    pub start_controller: bool,
    pub snapshot: ResidencySnapshot,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReleaseDecision {
    pub drain_controller: bool,
    pub drained_identity: Option<ControllerIdentity>,
    pub snapshot: ResidencySnapshot,
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum ResidencyError {
    #[error("holder identity fields must be non-empty")]
    InvalidHolder,
    #[error("controller identity fields and startup generation must be non-empty")]
    InvalidController,
    #[error("holder lease must expire after current time")]
    InvalidExpiry,
    #[error("controller identity does not match active controller")]
    ControllerMismatch,
    #[error("holder credential does not match active lease")]
    CredentialMismatch,
    #[error("holder lease is not active")]
    HolderMissing,
    #[error("holder lease has expired")]
    HolderExpired,
}

#[derive(Debug, Default)]
pub struct ResidencyRegistry {
    controller: Option<ControllerIdentity>,
    holders: BTreeMap<(HolderKind, String), HolderLease>,
}

impl ResidencyRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn snapshot(&self) -> ResidencySnapshot {
        let mut hub_holders = 0;
        let mut coderight_daemon_holders = 0;
        for kind in self.holders.keys().map(|(kind, _)| kind) {
            match kind {
                HolderKind::Hub => hub_holders += 1,
                HolderKind::CodeRightDaemon => coderight_daemon_holders += 1,
            }
        }
        ResidencySnapshot {
            controller: self.controller.clone(),
            hub_holders,
            coderight_daemon_holders,
        }
    }

    pub fn acquire(
        &mut self,
        controller: ControllerIdentity,
        holder: AuthenticatedHolder,
        now_ms: u64,
        expires_at_ms: u64,
    ) -> Result<AcquireDecision, ResidencyError> {
        validate_controller(&controller)?;
        validate_holder(&holder)?;
        if expires_at_ms <= now_ms {
            return Err(ResidencyError::InvalidExpiry);
        }
        if self.controller.as_ref().is_some_and(|active| active != &controller) {
            return Err(ResidencyError::ControllerMismatch);
        }
        // Do not let an acquire race the expiry worker after the final lease
        // has already expired.  Keeping the expired holder in place leaves
        // the controller visible to `reconcile_expired`, which records the
        // loss and closes lifecycle admission.  Purging it here would make
        // this acquire look like a fresh first holder and resurrect a
        // controller whose final holder was lost.
        if !self.holders.is_empty()
            && self
                .holders
                .values()
                .all(|lease| lease.expires_at_ms <= now_ms)
        {
            return Err(ResidencyError::HolderExpired);
        }
        self.reconcile_expired(now_ms);

        let key = (holder.kind, holder.holder_id);
        if self
            .holders
            .get(&key)
            .is_some_and(|lease| lease.credential_id != holder.credential_id)
        {
            return Err(ResidencyError::CredentialMismatch);
        }

        let start_controller = self.holders.is_empty();
        self.controller.get_or_insert(controller);
        self.holders.insert(
            key,
            HolderLease {
                credential_id: holder.credential_id,
                expires_at_ms,
            },
        );
        Ok(AcquireDecision {
            start_controller,
            snapshot: self.snapshot(),
        })
    }

    pub fn renew(
        &mut self,
        holder: &AuthenticatedHolder,
        now_ms: u64,
        expires_at_ms: u64,
    ) -> Result<ResidencySnapshot, ResidencyError> {
        validate_holder(holder)?;
        if expires_at_ms <= now_ms {
            return Err(ResidencyError::InvalidExpiry);
        }
        let lease = self
            .holders
            .get_mut(&(holder.kind, holder.holder_id.clone()))
            .ok_or(ResidencyError::HolderMissing)?;
        if lease.credential_id != holder.credential_id {
            return Err(ResidencyError::CredentialMismatch);
        }
        if lease.expires_at_ms <= now_ms {
            return Err(ResidencyError::HolderExpired);
        }
        lease.expires_at_ms = expires_at_ms;
        Ok(self.snapshot())
    }

    pub fn release(
        &mut self,
        holder: &AuthenticatedHolder,
    ) -> Result<ReleaseDecision, ResidencyError> {
        validate_holder(holder)?;
        let key = (holder.kind, holder.holder_id.clone());
        if let Some(lease) = self.holders.get(&key) {
            if lease.credential_id != holder.credential_id {
                return Err(ResidencyError::CredentialMismatch);
            }
            self.holders.remove(&key);
        }
        Ok(self.finalize_if_unheld())
    }

    pub fn reconcile_expired(&mut self, now_ms: u64) -> ReleaseDecision {
        self.holders.retain(|_, lease| lease.expires_at_ms > now_ms);
        self.finalize_if_unheld()
    }

    fn finalize_if_unheld(&mut self) -> ReleaseDecision {
        let drained_identity = if self.holders.is_empty() {
            self.controller.take()
        } else {
            None
        };
        ReleaseDecision {
            drain_controller: drained_identity.is_some(),
            drained_identity,
            snapshot: self.snapshot(),
        }
    }
}

fn validate_holder(holder: &AuthenticatedHolder) -> Result<(), ResidencyError> {
    if holder.holder_id.trim().is_empty() || holder.credential_id.trim().is_empty() {
        Err(ResidencyError::InvalidHolder)
    } else {
        Ok(())
    }
}

fn validate_controller(controller: &ControllerIdentity) -> Result<(), ResidencyError> {
    if controller.installation_id.trim().is_empty()
        || controller.cortex_store_id.trim().is_empty()
        || controller.release_generation.trim().is_empty()
        || controller.startup_generation == 0
    {
        Err(ResidencyError::InvalidController)
    } else {
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Canonical loopback-auth V1 signer.
//
// This is the ONE SDK-owned signer for the local loopback HMAC transport
// binding client, runtime `serve.rs`, and every route-authorized caller
// (tray `snapshot.rs`, Hub `dashboard_connection.rs`, CLI resident
// forwarder, CodeRight `native.rs` adapter). No other crate may hand-roll
// or diverge from this byte encoding, header profile, route list, or
// replay-capacity accounting. The payload never carries a reusable
// credential; the HMAC proves integrity and response authenticity only.
// ---------------------------------------------------------------------------

use hmac::{Hmac, KeyInit, Mac};
use sha2::{Digest, Sha256};

type HmacSha256 = Hmac<Sha256>;

/// Domain string prefixed to every canonical loopback-auth V1 frame.
pub const LOOPBACK_AUTH_DOMAIN: &str = "membrane.loopback.auth.v1";
pub const LOOPBACK_AUTH_DOMAIN_LEN: u32 = 25;
pub const LOOPBACK_AUTH_REQUEST_KIND: u8 = 1;
pub const LOOPBACK_AUTH_RESPONSE_KIND: u8 = 2;

/// Nonce/expiry bounds from the canonical contract.
pub const LOOPBACK_NONCE_OCTETS: usize = 32;
pub const LOOPBACK_MAX_EXPIRY_SECS: u64 = 30;

/// Replay admission bounds: at most 4096 unexpired nonces total, 384
/// reserved for renew/release, so general traffic may occupy at most 3712.
pub const LOOPBACK_REPLAY_CAPACITY: usize = 4096;
pub const LOOPBACK_REPLAY_RESERVED: usize = 384;
pub const LOOPBACK_REPLAY_GENERAL_MAX: usize = LOOPBACK_REPLAY_CAPACITY - LOOPBACK_REPLAY_RESERVED;

/// Canonical identity quintuple carried in both request and response frames.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoopbackIdentityFields {
    pub installation_id: String,
    pub cortex_store_id: String,
    pub release_generation: String,
    pub startup_generation: u64,
    pub stable_install_root: String,
}

/// Fields for request kind 1, in canonical order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoopbackRequestFields {
    pub method: String,
    pub target: String,
    pub host: String,
    pub content_type: String,
    pub body_sha256: [u8; 32],
    pub identity: LoopbackIdentityFields,
    pub nonce: [u8; LOOPBACK_NONCE_OCTETS],
    pub expiry_unix_secs: u64,
}

/// Fields for response kind 2, in canonical order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoopbackResponseFields {
    pub nonce: [u8; LOOPBACK_NONCE_OCTETS],
    pub status: u16,
    pub body_sha256: [u8; 32],
    pub identity: LoopbackIdentityFields,
}

fn push_field(buf: &mut Vec<u8>, octets: &[u8]) {
    buf.extend_from_slice(&(octets.len() as u32).to_be_bytes());
    buf.extend_from_slice(octets);
}

fn push_prefix(buf: &mut Vec<u8>, kind: u8) {
    buf.extend_from_slice(&LOOPBACK_AUTH_DOMAIN_LEN.to_be_bytes());
    buf.extend_from_slice(LOOPBACK_AUTH_DOMAIN.as_bytes());
    buf.push(kind);
}

fn push_identity(buf: &mut Vec<u8>, identity: &LoopbackIdentityFields) {
    push_field(buf, identity.installation_id.as_bytes());
    push_field(buf, identity.cortex_store_id.as_bytes());
    push_field(buf, identity.release_generation.as_bytes());
    buf.extend_from_slice(&identity.startup_generation.to_be_bytes());
    push_field(buf, identity.stable_install_root.as_bytes());
}

/// Canonical byte encoding for request kind 1: exact field order, no NUL
/// terminator, `u32be(len) || octets` for every variable-length field and
/// `u64be` for `startupGeneration` and `expiry`.
pub fn encode_loopback_request(fields: &LoopbackRequestFields) -> Vec<u8> {
    let mut buf = Vec::new();
    push_prefix(&mut buf, LOOPBACK_AUTH_REQUEST_KIND);
    push_field(&mut buf, fields.method.as_bytes());
    push_field(&mut buf, fields.target.as_bytes());
    push_field(&mut buf, fields.host.as_bytes());
    push_field(&mut buf, fields.content_type.as_bytes());
    push_field(&mut buf, &fields.body_sha256);
    push_field(&mut buf, fields.identity.installation_id.as_bytes());
    push_field(&mut buf, fields.identity.cortex_store_id.as_bytes());
    push_field(&mut buf, fields.identity.release_generation.as_bytes());
    buf.extend_from_slice(&fields.identity.startup_generation.to_be_bytes());
    push_field(&mut buf, fields.identity.stable_install_root.as_bytes());
    push_field(&mut buf, &fields.nonce);
    buf.extend_from_slice(&fields.expiry_unix_secs.to_be_bytes());
    buf
}

/// Canonical byte encoding for response kind 2: request nonce, status
/// (`u16be`), raw-response SHA-256, then the five identity fields.
pub fn encode_loopback_response(fields: &LoopbackResponseFields) -> Vec<u8> {
    let mut buf = Vec::new();
    push_prefix(&mut buf, LOOPBACK_AUTH_RESPONSE_KIND);
    push_field(&mut buf, &fields.nonce);
    buf.extend_from_slice(&fields.status.to_be_bytes());
    push_field(&mut buf, &fields.body_sha256);
    push_identity(&mut buf, &fields.identity);
    buf
}

/// Header names allowed in the canonical loopback-auth V1 profile. Exactly
/// one of the semantic auth headers must be present; `Authorization`,
/// duplicates, and unknown semantic headers are rejected by callers using
/// this list, along with any query/fragment on the target.
pub const LOOPBACK_HEADER_VERSION: &str = "x-membrane-loopback-version";
pub const LOOPBACK_HEADER_NONCE: &str = "x-membrane-loopback-nonce";
pub const LOOPBACK_HEADER_EXPIRY: &str = "x-membrane-loopback-expiry";
pub const LOOPBACK_HEADER_INSTALLATION_ID: &str = "x-membrane-loopback-installation-id";
pub const LOOPBACK_HEADER_CORTEX_STORE_ID: &str = "x-membrane-loopback-cortex-store-id";
pub const LOOPBACK_HEADER_RELEASE_GENERATION: &str = "x-membrane-loopback-release-generation";
pub const LOOPBACK_HEADER_STARTUP_GENERATION: &str = "x-membrane-loopback-startup-generation";
pub const LOOPBACK_HEADER_STABLE_INSTALL_ROOT: &str = "x-membrane-loopback-stable-install-root";
pub const LOOPBACK_HEADER_PROOF: &str = "x-membrane-loopback-proof";

/// The complete required semantic-header set: version, nonce, expiry, five
/// identity fields, and proof — exactly one of each.
pub const LOOPBACK_REQUIRED_HEADERS: &[&str] = &[
    LOOPBACK_HEADER_VERSION,
    LOOPBACK_HEADER_NONCE,
    LOOPBACK_HEADER_EXPIRY,
    LOOPBACK_HEADER_INSTALLATION_ID,
    LOOPBACK_HEADER_CORTEX_STORE_ID,
    LOOPBACK_HEADER_RELEASE_GENERATION,
    LOOPBACK_HEADER_STARTUP_GENERATION,
    LOOPBACK_HEADER_STABLE_INSTALL_ROOT,
    LOOPBACK_HEADER_PROOF,
];

/// Transport-level headers the profile additionally allows alongside the
/// required semantic headers.
pub const LOOPBACK_ALLOWED_TRANSPORT_HEADERS: &[&str] =
    &["host", "content-length", "connection", "content-type"];

/// Validate an inbound/outbound header set against the canonical profile.
/// `headers` are lower-cased header names paired with their raw values;
/// callers must lower-case before calling. Rejects duplicate/unknown
/// semantic headers, `Authorization`, and any header outside the allowed
/// transport set. Query/fragment and body-framing (chunked/non-JSON)
/// rejection is enforced by the caller against `target`/body directly,
/// since this profile only owns the header surface.
pub fn validate_loopback_header_profile(
    headers: &[(String, String)],
) -> Result<(), ClientError> {
    let mut seen_required: BTreeMap<&'static str, u32> = LOOPBACK_REQUIRED_HEADERS
        .iter()
        .map(|name| (*name, 0))
        .collect();
    for (name, _value) in headers {
        let lowered = name.to_ascii_lowercase();
        if lowered == "authorization" {
            return Err(ClientError::Denied {
                message: "loopback-auth profile rejects Authorization header".into(),
            });
        }
        if let Some(count) = seen_required.get_mut(lowered.as_str()) {
            *count += 1;
            continue;
        }
        if LOOPBACK_ALLOWED_TRANSPORT_HEADERS.contains(&lowered.as_str()) {
            continue;
        }
        return Err(ClientError::Denied {
            message: format!("loopback-auth profile rejects unknown header {lowered}"),
        });
    }
    if seen_required.values().any(|count| *count != 1) {
        return Err(ClientError::InvalidRequest {
            message: "loopback-auth profile requires exactly one of each semantic header".into(),
        });
    }
    Ok(())
}

fn hex_encode(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
    out
}

fn hex_decode<const N: usize>(value: &str, label: &str) -> Result<[u8; N], ClientError> {
    if value.len() != N * 2 || !value.bytes().all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase()) {
        return Err(ClientError::InvalidRequest {
            message: format!("loopback-auth {label} must be {N}-byte lowercase hex"),
        });
    }
    let mut out = [0u8; N];
    for (index, pair) in value.as_bytes().chunks_exact(2).enumerate() {
        let high = (pair[0] as char).to_digit(16).unwrap() as u8;
        let low = (pair[1] as char).to_digit(16).unwrap() as u8;
        out[index] = (high << 4) | low;
    }
    Ok(out)
}

fn header_value<'a>(headers: &'a [(String, String)], name: &str) -> Result<&'a str, ClientError> {
    headers.iter().find_map(|(key, value)| {
        key.eq_ignore_ascii_case(name).then_some(value.as_str())
    }).ok_or_else(|| ClientError::InvalidRequest {
        message: format!("loopback-auth missing header {name}"),
    })
}

fn body_digest(body: &[u8]) -> [u8; 32] {
    Sha256::digest(body).into()
}

fn canonical_host(host: &str) -> Result<String, ClientError> {
    let normalized = host.trim().to_ascii_lowercase();
    if normalized.is_empty() || !normalized.is_ascii() || normalized.chars().any(char::is_whitespace)
    {
        return Err(ClientError::InvalidRequest { message: "loopback-auth host is not normalized ASCII".into() });
    }
    Ok(normalized)
}

fn validate_request_shape(method: &str, target: &str, content_type: &str, body: &[u8]) -> Result<(), ClientError> {
    if !method.is_ascii() || !target.is_ascii() || !content_type.is_ascii()
        || target.contains('?') || target.contains('#') {
        return Err(ClientError::InvalidRequest { message: "loopback-auth request has invalid method, target, or content type".into() });
    }
    if !loopback_routes::is_authorized(method, target) {
        return Err(ClientError::Denied { message: format!("loopback-auth route is not authorized: {method} {target}") });
    }
    if method == "POST" && content_type != "application/json" {
        return Err(ClientError::InvalidRequest { message: "loopback-auth POST requires application/json".into() });
    }
    if method == "GET" && !content_type.is_empty() {
        return Err(ClientError::InvalidRequest { message: "loopback-auth GET content type must be empty".into() });
    }
    if method == "GET" && !body.is_empty() {
        return Err(ClientError::InvalidRequest { message: "loopback-auth GET body must be empty".into() });
    }
    if method == "POST" && serde_json::from_slice::<Value>(body).is_err() {
        return Err(ClientError::InvalidRequest { message: "loopback-auth body must be valid JSON".into() });
    }
    Ok(())
}

/// Construct the complete canonical request header set. `body` is the exact
/// bytes sent on wire; callers must preserve it unchanged.
pub fn build_loopback_request_headers(
    signer: &LoopbackAuthSigner,
    identity: &LoopbackIdentityFields,
    method: &str,
    target: &str,
    host: &str,
    content_type: &str,
    body: &[u8],
    nonce: [u8; LOOPBACK_NONCE_OCTETS],
    expiry_unix_secs: u64,
) -> Result<Vec<(String, String)>, ClientError> {
    let host = canonical_host(host)?;
    validate_request_shape(method, target, content_type, body)?;
    if expiry_unix_secs == 0 || identity.installation_id.is_empty() || identity.cortex_store_id.is_empty()
        || identity.release_generation.is_empty() || identity.stable_install_root.is_empty()
    {
        return Err(ClientError::InvalidRequest { message: "loopback-auth identity or expiry is empty".into() });
    }
    let fields = LoopbackRequestFields {
        method: method.into(), target: target.into(), host: host.clone(), content_type: content_type.into(),
        body_sha256: body_digest(body), identity: identity.clone(), nonce, expiry_unix_secs,
    };
    let proof = signer.sign_request(&fields)?;
    let mut headers = vec![
        (LOOPBACK_HEADER_VERSION.into(), "1".into()),
        (LOOPBACK_HEADER_NONCE.into(), hex_encode(&nonce)),
        (LOOPBACK_HEADER_EXPIRY.into(), expiry_unix_secs.to_string()),
        (LOOPBACK_HEADER_INSTALLATION_ID.into(), identity.installation_id.clone()),
        (LOOPBACK_HEADER_CORTEX_STORE_ID.into(), identity.cortex_store_id.clone()),
        (LOOPBACK_HEADER_RELEASE_GENERATION.into(), identity.release_generation.clone()),
        (LOOPBACK_HEADER_STARTUP_GENERATION.into(), identity.startup_generation.to_string()),
        (LOOPBACK_HEADER_STABLE_INSTALL_ROOT.into(), identity.stable_install_root.clone()),
        (LOOPBACK_HEADER_PROOF.into(), hex_encode(&proof)),
        ("host".into(), host),
        ("content-length".into(), body.len().to_string()),
        ("connection".into(), "close".into()),
    ];
    if !content_type.is_empty() { headers.push(("content-type".into(), content_type.into())); }
    Ok(headers)
}

/// Verify request headers against method, exact target, normalized host, and
/// exact raw body, returning parsed fields only after HMAC verification.
pub fn verify_loopback_request_headers(
    signer: &LoopbackAuthSigner,
    headers: &[(String, String)], method: &str, target: &str, host: &str,
    content_type: &str, body: &[u8], expected_identity: &LoopbackIdentityFields,
    now_unix_secs: u64,
) -> Result<LoopbackRequestFields, ClientError> {
    validate_loopback_header_profile(headers)?;
    let normalized_host = canonical_host(host)?;
    validate_request_shape(method, target, content_type, body)?;
    if header_value(headers, "host")? != normalized_host { return Err(ClientError::Denied { message: "loopback-auth host mismatch".into() }); }
    if header_value(headers, "connection")? != "close" { return Err(ClientError::InvalidRequest { message: "loopback-auth connection must be close".into() }); }
    if method == "POST" && header_value(headers, "content-type")? != "application/json" { return Err(ClientError::InvalidRequest { message: "loopback-auth content type mismatch".into() }); }
    if method == "GET" && headers.iter().any(|(name, _)| name.eq_ignore_ascii_case("content-type")) { return Err(ClientError::InvalidRequest { message: "loopback-auth GET cannot carry content type".into() }); }
    if let Ok(length) = header_value(headers, "content-length")?.parse::<usize>() {
        if length != body.len() { return Err(ClientError::InvalidRequest { message: "loopback-auth content length mismatch".into() }); }
    } else { return Err(ClientError::InvalidRequest { message: "loopback-auth invalid content length".into() }); }
    if header_value(headers, LOOPBACK_HEADER_VERSION)? != "1" { return Err(ClientError::Denied { message: "loopback-auth unsupported profile version".into() }); }
    let expiry = header_value(headers, LOOPBACK_HEADER_EXPIRY)?.parse::<u64>().map_err(|_| ClientError::InvalidRequest { message: "loopback-auth invalid expiry".into() })?;
    if expiry <= now_unix_secs || expiry.saturating_sub(now_unix_secs) > LOOPBACK_MAX_EXPIRY_SECS { return Err(ClientError::Denied { message: "loopback-auth expired or overlong expiry".into() }); }
    let startup_generation = header_value(headers, LOOPBACK_HEADER_STARTUP_GENERATION)?.parse::<u64>().map_err(|_| ClientError::InvalidRequest { message: "loopback-auth invalid startup generation".into() })?;
    let nonce = hex_decode::<LOOPBACK_NONCE_OCTETS>(header_value(headers, LOOPBACK_HEADER_NONCE)?, "nonce")?;
    let fields = LoopbackRequestFields { method: method.into(), target: target.into(), host: normalized_host, content_type: content_type.into(), body_sha256: body_digest(body), identity: LoopbackIdentityFields { installation_id: header_value(headers, LOOPBACK_HEADER_INSTALLATION_ID)?.into(), cortex_store_id: header_value(headers, LOOPBACK_HEADER_CORTEX_STORE_ID)?.into(), release_generation: header_value(headers, LOOPBACK_HEADER_RELEASE_GENERATION)?.into(), startup_generation, stable_install_root: header_value(headers, LOOPBACK_HEADER_STABLE_INSTALL_ROOT)?.into() }, nonce, expiry_unix_secs: expiry };
    if &fields.identity != expected_identity { return Err(ClientError::Denied { message: "loopback-auth identity mismatch".into() }); }
    signer.verify_request(&fields, &hex_decode::<32>(header_value(headers, LOOPBACK_HEADER_PROOF)?, "proof")?)?;
    Ok(fields)
}

/// Construct response headers bound to request nonce, status, raw response,
/// identity, and request expiry.
pub fn build_loopback_response_headers(
    signer: &LoopbackAuthSigner, identity: &LoopbackIdentityFields,
    nonce: [u8; LOOPBACK_NONCE_OCTETS], status: u16, body: &[u8], expiry_unix_secs: u64,
) -> Result<Vec<(String, String)>, ClientError> {
    let fields = LoopbackResponseFields { nonce, status, body_sha256: body_digest(body), identity: identity.clone() };
    let proof = signer.sign_response(&fields)?;
    Ok(vec![(LOOPBACK_HEADER_VERSION.into(), "1".into()), (LOOPBACK_HEADER_NONCE.into(), hex_encode(&nonce)), (LOOPBACK_HEADER_EXPIRY.into(), expiry_unix_secs.to_string()), (LOOPBACK_HEADER_INSTALLATION_ID.into(), identity.installation_id.clone()), (LOOPBACK_HEADER_CORTEX_STORE_ID.into(), identity.cortex_store_id.clone()), (LOOPBACK_HEADER_RELEASE_GENERATION.into(), identity.release_generation.clone()), (LOOPBACK_HEADER_STARTUP_GENERATION.into(), identity.startup_generation.to_string()), (LOOPBACK_HEADER_STABLE_INSTALL_ROOT.into(), identity.stable_install_root.clone()), (LOOPBACK_HEADER_PROOF.into(), hex_encode(&proof)), ("connection".into(), "close".into()), ("content-length".into(), body.len().to_string())])
}

/// Verify response proof and binding to request nonce, status, raw body,
/// identity, and request expiry.
pub fn verify_loopback_response_headers(
    signer: &LoopbackAuthSigner, headers: &[(String, String)], request: &LoopbackRequestFields,
    status: u16, body: &[u8], expected_identity: &LoopbackIdentityFields, now_unix_secs: u64,
) -> Result<(), ClientError> {
    validate_loopback_header_profile(headers)?;
    if header_value(headers, "connection")? != "close" { return Err(ClientError::InvalidRequest { message: "loopback-auth response connection must be close".into() }); }
    if header_value(headers, LOOPBACK_HEADER_VERSION)? != "1" || header_value(headers, LOOPBACK_HEADER_EXPIRY)? != request.expiry_unix_secs.to_string() || request.expiry_unix_secs <= now_unix_secs { return Err(ClientError::Denied { message: "loopback-auth response expiry mismatch".into() }); }
    if header_value(headers, "content-length")?.parse::<usize>().ok() != Some(body.len()) { return Err(ClientError::InvalidRequest { message: "loopback-auth response content length mismatch".into() }); }
    let nonce = hex_decode::<LOOPBACK_NONCE_OCTETS>(header_value(headers, LOOPBACK_HEADER_NONCE)?, "nonce")?;
    if nonce != request.nonce { return Err(ClientError::Denied { message: "loopback-auth response nonce mismatch".into() }); }
    let identity = LoopbackIdentityFields { installation_id: header_value(headers, LOOPBACK_HEADER_INSTALLATION_ID)?.into(), cortex_store_id: header_value(headers, LOOPBACK_HEADER_CORTEX_STORE_ID)?.into(), release_generation: header_value(headers, LOOPBACK_HEADER_RELEASE_GENERATION)?.into(), startup_generation: header_value(headers, LOOPBACK_HEADER_STARTUP_GENERATION)?.parse().map_err(|_| ClientError::InvalidRequest { message: "loopback-auth invalid startup generation".into() })?, stable_install_root: header_value(headers, LOOPBACK_HEADER_STABLE_INSTALL_ROOT)?.into() };
    if &identity != expected_identity { return Err(ClientError::Denied { message: "loopback-auth response identity mismatch".into() }); }
    let fields = LoopbackResponseFields { nonce, status, body_sha256: body_digest(body), identity };
    signer.verify_response(&fields, &hex_decode::<32>(header_value(headers, LOOPBACK_HEADER_PROOF)?, "proof")?)
}

/// Canonical route authority. Derived only from `memory_backend.rs` route
/// constants plus the CodeRight `native.rs` holder/dashboard routes; no
/// third route list is valid.
pub mod loopback_routes {
    use crate::memory_backend::{
        ACTIVITY, DELETE, FEDERATE, GET, HEALTH, LIST, METRICS, PUT, RECALL, REMEMBER,
        REMEMBER_CONSOLIDATED, SCOPES, SEARCH, USE,
    };

    pub const GET_ROUTES: &[&str] = &[HEALTH, METRICS, ACTIVITY];
    pub const POST_ROUTES: &[&str] = &[
        DELETE,
        FEDERATE,
        GET,
        LIST,
        PUT,
        RECALL,
        REMEMBER,
        REMEMBER_CONSOLIDATED,
        SCOPES,
        SEARCH,
        USE,
        "/resident-holder/acquire-permit",
        "/resident-holder",
    ];
    pub const HUB_SNAPSHOT_GET_ROUTE: &str = "/hub/snapshot";

    pub fn is_authorized(method: &str, target: &str) -> bool {
        match method {
            "GET" => GET_ROUTES.contains(&target) || target == HUB_SNAPSHOT_GET_ROUTE,
            "POST" => POST_ROUTES.contains(&target),
            _ => false,
        }
    }
}

/// The one SDK-owned canonical loopback-auth V1 signer. Holds only the
/// symmetric key; never transmits or persists a reusable credential in the
/// signed payload itself.
pub struct LoopbackAuthSigner {
    key: Vec<u8>,
}

impl LoopbackAuthSigner {
    pub fn new(key: Vec<u8>) -> Self {
        Self { key }
    }

    /// Decode the installed credential's canonical 64 lowercase hex form.
    pub fn from_hex_token(token: &str) -> Result<Self, ClientError> {
        Ok(Self::new(hex_decode::<32>(token, "api token")?.to_vec()))
    }
    fn mac(&self) -> Result<HmacSha256, ClientError> {
        HmacSha256::new_from_slice(&self.key).map_err(|_| ClientError::Internal {
            message: "loopback-auth signer key is invalid length".into(),
        })
    }

    /// Compute the HMAC-SHA256 proof tag over canonical bytes.
    pub fn sign(&self, canonical_bytes: &[u8]) -> Result<[u8; 32], ClientError> {
        let mut mac = self.mac()?;
        mac.update(canonical_bytes);
        let tag = mac.finalize().into_bytes();
        let mut out = [0u8; 32];
        out.copy_from_slice(&tag);
        Ok(out)
    }

    /// Constant-time verification of a proof tag against canonical bytes.
    pub fn verify(&self, canonical_bytes: &[u8], tag: &[u8]) -> Result<(), ClientError> {
        let mut mac = self.mac()?;
        mac.update(canonical_bytes);
        mac.verify_slice(tag).map_err(|_| ClientError::Denied {
            message: "loopback-auth proof verification failed".into(),
        })
    }

    pub fn sign_request(&self, fields: &LoopbackRequestFields) -> Result<[u8; 32], ClientError> {
        self.sign(&encode_loopback_request(fields))
    }

    pub fn sign_response(&self, fields: &LoopbackResponseFields) -> Result<[u8; 32], ClientError> {
        self.sign(&encode_loopback_response(fields))
    }

    pub fn verify_request(&self, fields: &LoopbackRequestFields, tag: &[u8]) -> Result<(), ClientError> {
        self.verify(&encode_loopback_request(fields), tag)
    }

    pub fn verify_response(&self, fields: &LoopbackResponseFields, tag: &[u8]) -> Result<(), ClientError> {
        self.verify(&encode_loopback_response(fields), tag)
    }

    /// Generate a fresh 32-octet nonce via `getrandom`. Never reused; the
    /// replay cache is the sole authority on admission.
    pub fn generate_nonce() -> Result<[u8; LOOPBACK_NONCE_OCTETS], ClientError> {
        let mut nonce = [0u8; LOOPBACK_NONCE_OCTETS];
        getrandom::fill(&mut nonce).map_err(|error| ClientError::Internal {
            message: format!("generate loopback-auth nonce: {error}"),
        })?;
        Ok(nonce)
    }

    /// Cap a requested expiry (seconds from now) at the canonical 30 s
    /// ceiling.
    pub fn bounded_expiry(now_unix_secs: u64, requested_ttl_secs: u64) -> u64 {
        now_unix_secs.saturating_add(requested_ttl_secs.min(LOOPBACK_MAX_EXPIRY_SECS))
    }
}

/// Replay budget classification used to admit nonces without ever evicting
/// a live one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoopbackReplayPartition {
    /// Acquire/other general traffic; bounded to 3712 concurrent slots.
    General,
    /// Renew/release; drawn from the protected 384-slot reserve so a
    /// live holder can always renew or release even when general traffic
    /// has saturated its partition.
    Reserved,
}

/// Bounded replay-admission cache: at most 4096 unexpired nonces, split
/// 3712 general / 384 reserved. Full partitions return a typed
/// retry-after / `ReplayAdmissionSaturated` outcome rather than evicting a
/// live nonce. Reset only by constructing a new cache (callers reset on a
/// new `startupGeneration`).
#[derive(Debug, Default)]
pub struct LoopbackReplayCache {
    general: BTreeMap<[u8; LOOPBACK_NONCE_OCTETS], u64>,
    reserved: BTreeMap<[u8; LOOPBACK_NONCE_OCTETS], u64>,
}

impl LoopbackReplayCache {
    pub fn new() -> Self {
        Self::default()
    }

    fn reconcile(&mut self, now_unix_secs: u64) {
        self.general.retain(|_, expiry| *expiry > now_unix_secs);
        self.reserved.retain(|_, expiry| *expiry > now_unix_secs);
    }

    /// Admit one nonce into the given partition. Returns a typed
    /// `ClientError::Protocol` with code `replay_admission_saturated` and a
    /// `retry_after_ms` detail when the partition is full; never evicts a
    /// live nonce to make room.
    pub fn admit(
        &mut self,
        partition: LoopbackReplayPartition,
        nonce: [u8; LOOPBACK_NONCE_OCTETS],
        expiry_unix_secs: u64,
        now_unix_secs: u64,
    ) -> Result<(), ClientError> {
        self.reconcile(now_unix_secs);
        if self.general.contains_key(&nonce) || self.reserved.contains_key(&nonce) {
            return Err(ClientError::Protocol {
                code: "replay_nonce_seen".into(),
                message: "loopback-auth nonce was already admitted".into(),
                details: Map::new(),
            });
        }
        let (table, limit) = match partition {
            LoopbackReplayPartition::General => (&mut self.general, LOOPBACK_REPLAY_GENERAL_MAX),
            LoopbackReplayPartition::Reserved => (&mut self.reserved, LOOPBACK_REPLAY_RESERVED),
        };
        if table.len() >= limit {
            let retry_after_ms = table.values().copied().min().unwrap_or(expiry_unix_secs)
                .saturating_sub(now_unix_secs)
                .saturating_mul(1000);
            let mut details = Map::new();
            details.insert("retryAfterMs".into(), Value::from(retry_after_ms));
            details.insert(
                "readiness".into(),
                Value::from("ReplayAdmissionSaturated"),
            );
            return Err(ClientError::Protocol {
                code: "replay_admission_saturated".into(),
                message: "loopback-auth replay partition is at capacity".into(),
                details,
            });
        }
        table.insert(nonce, expiry_unix_secs);
        Ok(())
    }

    pub fn general_len(&self) -> usize {
        self.general.len()
    }

    pub fn reserved_len(&self) -> usize {
        self.reserved.len()
    }
    pub fn snapshot(&mut self, now_unix_secs: u64) -> LoopbackReplaySnapshot {
        self.reconcile(now_unix_secs);
        LoopbackReplaySnapshot {
            general: LoopbackReplayPartitionSnapshot::new(self.general.len(), LOOPBACK_REPLAY_GENERAL_MAX, &self.general),
            reserved: LoopbackReplayPartitionSnapshot::new(self.reserved.len(), LOOPBACK_REPLAY_RESERVED, &self.reserved),
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoopbackReplaySnapshot { pub general: LoopbackReplayPartitionSnapshot, pub reserved: LoopbackReplayPartitionSnapshot }
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoopbackReplayPartitionSnapshot { pub active: usize, pub limit: usize, pub expiring_entries: usize, pub earliest_expiry_unix_secs: Option<u64> }
impl LoopbackReplayPartitionSnapshot {
    fn new(active: usize, limit: usize, entries: &BTreeMap<[u8; LOOPBACK_NONCE_OCTETS], u64>) -> Self {
        Self { active, limit, expiring_entries: entries.len(), earliest_expiry_unix_secs: entries.values().copied().min() }
    }
}

#[cfg(test)]
mod loopback_auth_tests {
    #[test]
    fn replay_snapshot_recovers_protected_capacity_at_expiry() {
        let mut cache = super::LoopbackReplayCache::new();
        for index in 0..super::LOOPBACK_REPLAY_RESERVED {
            let mut nonce = [0; 32];
            nonce[..8].copy_from_slice(&(index as u64).to_be_bytes());
            cache.admit(super::LoopbackReplayPartition::Reserved, nonce, 20, 10).unwrap();
        }
        let full = cache.snapshot(19);
        assert_eq!(full.reserved.active, full.reserved.limit);
        assert_eq!(full.reserved.earliest_expiry_unix_secs, Some(20));
        let recovered = cache.snapshot(20);
        assert_eq!(recovered.reserved.active, 0);
        assert_eq!(recovered.reserved.earliest_expiry_unix_secs, None);
    }
    #[test]
    fn duplicate_nonce_refused_in_both_partitions_until_expiry() {
        let mut cache = super::LoopbackReplayCache::new();
        let nonce = [9; 32];
        cache.admit(super::LoopbackReplayPartition::General, nonce, 20, 10).unwrap();
        for partition in [super::LoopbackReplayPartition::General, super::LoopbackReplayPartition::Reserved] {
            assert!(matches!(cache.admit(partition, nonce, 20, 10), Err(super::ClientError::Protocol { code, .. }) if code == "replay_nonce_seen"));
        }
        cache.admit(super::LoopbackReplayPartition::Reserved, nonce, 30, 20).unwrap();
    }
    use super::*;

    fn identity() -> LoopbackIdentityFields {
        LoopbackIdentityFields {
            installation_id: "install-1".into(),
            cortex_store_id: "store-1".into(),
            release_generation: "release-1".into(),
            startup_generation: 7,
            stable_install_root: "C:/canonical".into(),
        }
    }

    #[test]
    fn request_encoding_has_no_nul_terminator_and_starts_with_domain_prefix() {
        let fields = LoopbackRequestFields {
            method: "POST".into(),
            target: "/remember".into(),
            host: "127.0.0.1".into(),
            content_type: "application/json".into(),
            body_sha256: [1u8; 32],
            identity: identity(),
            nonce: [2u8; 32],
            expiry_unix_secs: 30,
        };
        let bytes = encode_loopback_request(&fields);
        assert_eq!(&bytes[0..4], &25u32.to_be_bytes());
        assert_eq!(&bytes[4..29], LOOPBACK_AUTH_DOMAIN.as_bytes());
        assert_eq!(bytes[29], LOOPBACK_AUTH_REQUEST_KIND);
        assert!(!bytes.contains(&0u8) || bytes.iter().any(|byte| *byte != 0));
    }

    #[test]
    fn signer_round_trips_and_rejects_tampered_bytes() {
        let signer = LoopbackAuthSigner::new(vec![9u8; 32]);
        let fields = LoopbackResponseFields {
            nonce: [3u8; 32],
            status: 200,
            body_sha256: [4u8; 32],
            identity: identity(),
        };
        let bytes = encode_loopback_response(&fields);
        let tag = signer.sign(&bytes).unwrap();
        assert!(signer.verify(&bytes, &tag).is_ok());
        let mut tampered = bytes.clone();
        tampered[0] ^= 0xFF;
        assert!(signer.verify(&tampered, &tag).is_err());
    }

    #[test]
    fn header_profile_requires_exactly_one_of_each_and_rejects_authorization() {
        let mut headers: Vec<(String, String)> = LOOPBACK_REQUIRED_HEADERS
            .iter()
            .map(|name| (name.to_string(), "value".to_string()))
            .collect();
        headers.push(("host".into(), "127.0.0.1".into()));
        assert!(validate_loopback_header_profile(&headers).is_ok());

        let mut with_auth = headers.clone();
        with_auth.push(("Authorization".into(), "Bearer x".into()));
        assert!(validate_loopback_header_profile(&with_auth).is_err());

        let mut duplicated = headers.clone();
        duplicated.push((LOOPBACK_HEADER_NONCE.into(), "again".into()));
        assert!(validate_loopback_header_profile(&duplicated).is_err());
    }

    #[test]
    fn replay_cache_reserves_partition_and_saturates_without_eviction() {
        let mut cache = LoopbackReplayCache::new();
        for index in 0..LOOPBACK_REPLAY_GENERAL_MAX {
            let mut nonce = [0u8; 32];
            nonce[0..8].copy_from_slice(&(index as u64).to_be_bytes());
            cache
                .admit(LoopbackReplayPartition::General, nonce, 1_000, 0)
                .unwrap();
        }
        let mut overflow = [0xFFu8; 32];
        let result = cache.admit(LoopbackReplayPartition::General, overflow, 1_000, 0);
        assert!(result.is_err());
        // Reserved partition remains available even though general is full.
        overflow[0] = 1;
        assert!(cache
            .admit(LoopbackReplayPartition::Reserved, overflow, 1_000, 0)
            .is_ok());
    }

    #[test]
    fn request_header_builder_and_verifier_bind_exact_raw_body_identity_and_route() {
        let signer = LoopbackAuthSigner::new(vec![9u8; 32]);
        let identity = identity();
        let body = br#"{"key":"value"}"#;
        let headers = build_loopback_request_headers(
            &signer, &identity, "POST", "/remember", "127.0.0.1", "application/json", body,
            [2u8; LOOPBACK_NONCE_OCTETS], 120,
        ).unwrap();
        let request = verify_loopback_request_headers(
            &signer, &headers, "POST", "/remember", "127.0.0.1", "application/json", body,
            &identity, 100,
        ).unwrap();
        assert_eq!(request.nonce, [2u8; LOOPBACK_NONCE_OCTETS]);
        assert!(verify_loopback_request_headers(
            &signer, &headers, "POST", "/remember", "127.0.0.1", "application/json", br#"{"key":"tampered"}"#,
            &identity, 100,
        ).is_err());
    }

    #[test]
    fn response_header_builder_and_verifier_bind_nonce_status_body_and_identity() {
        let signer = LoopbackAuthSigner::new(vec![9u8; 32]);
        let identity = identity();
        let body = br#"{"ok":true}"#;
        let request = LoopbackRequestFields {
            method: "POST".into(), target: "/remember".into(), host: "127.0.0.1".into(),
            content_type: "application/json".into(), body_sha256: body_digest(br#"{"key":"value"}"#),
            identity: identity.clone(), nonce: [2u8; LOOPBACK_NONCE_OCTETS], expiry_unix_secs: 120,
        };
        let headers = build_loopback_response_headers(&signer, &identity, request.nonce, 200, body, 120).unwrap();
        assert!(verify_loopback_response_headers(&signer, &headers, &request, 200, body, &identity, 100).is_ok());
        assert!(verify_loopback_response_headers(&signer, &headers, &request, 500, body, &identity, 100).is_err());
    }

    #[test]
    fn expiry_is_capped_at_thirty_seconds() {
        assert_eq!(LoopbackAuthSigner::bounded_expiry(0, 1000), LOOPBACK_MAX_EXPIRY_SECS);
        assert_eq!(LoopbackAuthSigner::bounded_expiry(100, 5), 105);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn controller() -> ControllerIdentity {
        ControllerIdentity {
            installation_id: "install-1".into(),
            cortex_store_id: "store-1".into(),
            release_generation: "release-1".into(),
            startup_generation: 1,
        }
    }

    fn holder(kind: HolderKind, id: &str) -> AuthenticatedHolder {
        AuthenticatedHolder {
            kind,
            holder_id: id.into(),
            credential_id: format!("credential-{id}"),
        }
    }

    #[test]
    fn all_four_holder_states_share_one_controller_and_drain_only_last() {
        let mut registry = ResidencyRegistry::new();
        let hub = holder(HolderKind::Hub, "hub-1");
        let coderight = holder(HolderKind::CodeRightDaemon, "coderight-1");
        assert!(registry.acquire(controller(), hub.clone(), 10, 100).unwrap().start_controller);
        assert!(!registry.acquire(controller(), coderight.clone(), 10, 100).unwrap().start_controller);
        assert_eq!((registry.snapshot().hub_holders, registry.snapshot().coderight_daemon_holders), (1, 1));
        assert!(!registry.release(&hub).unwrap().drain_controller);
        let neither = registry.release(&coderight).unwrap();
        assert!(neither.drain_controller);
        assert_eq!(neither.drained_identity, Some(controller()));
    }

    #[test]
    fn leases_are_idempotent_bounded_and_authenticated() {
        let mut registry = ResidencyRegistry::new();
        let hub = holder(HolderKind::Hub, "hub-1");
        assert!(registry.acquire(controller(), hub.clone(), 1, 10).unwrap().start_controller);
        assert!(!registry.acquire(controller(), hub.clone(), 2, 20).unwrap().start_controller);
        assert_eq!(registry.renew(&hub, 3, 30).unwrap().hub_holders, 1);
        let mut wrong = hub.clone();
        wrong.credential_id = "wrong".into();
        assert_eq!(registry.release(&wrong), Err(ResidencyError::CredentialMismatch));
        assert!(registry.reconcile_expired(30).drain_controller);
    }

    #[test]
    fn active_controller_rejects_split_identity() {
        let mut registry = ResidencyRegistry::new();
        registry.acquire(controller(), holder(HolderKind::Hub, "hub-1"), 1, 10).unwrap();
        let mut changed = controller();
        changed.cortex_store_id = "other-store".into();
        assert_eq!(
            registry.acquire(changed, holder(HolderKind::CodeRightDaemon, "coderight-1"), 1, 10),
            Err(ResidencyError::ControllerMismatch)
        );
    }
}
