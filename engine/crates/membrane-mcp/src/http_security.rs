//! Pure admission contract for optional Streamable HTTP MCP.
//!
//! This module opens no listener.  Hosts must call `admit` before dispatching
//! an HTTP request; stdio remains Membrane's default transport.
use serde::Serialize;
use std::net::IpAddr;

pub const DEFAULT_MAX_BODY_BYTES: usize = 1_048_576;
pub const DEFAULT_DEADLINE_MS: u64 = 15_000;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HttpAdmissionPolicy {
    pub installation_id: String,
    pub allowed_hosts: Vec<String>,
    pub allowed_origins: Vec<String>,
    pub expected_bearer_token: String,
    pub expected_session_binding: String,
    pub max_body_bytes: usize,
    pub max_deadline_ms: u64,
}

impl HttpAdmissionPolicy {
    pub fn local(installation_id: impl Into<String>, host: impl Into<String>, origin: impl Into<String>, bearer_token: impl Into<String>, session_binding: impl Into<String>) -> Self {
        Self {
            installation_id: installation_id.into(),
            allowed_hosts: vec![host.into()],
            allowed_origins: vec![origin.into()],
            expected_bearer_token: bearer_token.into(),
            expected_session_binding: session_binding.into(),
            max_body_bytes: DEFAULT_MAX_BODY_BYTES,
            max_deadline_ms: DEFAULT_DEADLINE_MS,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HttpAdmissionRequest<'a> {
    pub peer_ip: IpAddr,
    pub resolved_host_ip: IpAddr,
    pub host: &'a str,
    pub origin: &'a str,
    pub installation_id: &'a str,
    pub bearer_token: &'a str,
    pub session_binding: &'a str,
    pub body_bytes: usize,
    pub deadline_ms: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum HttpDenialCode {
    NonLoopbackPeer,
    DnsRebinding,
    HostNotAllowed,
    OriginNotAllowed,
    InstallationMismatch,
    MissingBearer,
    InvalidBearer,
    SessionMismatch,
    BodyTooLarge,
    DeadlineTooLong,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct HttpAdmissionReceipt {
    pub accepted: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub denial: Option<HttpDenialCode>,
    pub transport: &'static str,
}

impl HttpAdmissionReceipt {
    fn allow() -> Self { Self { accepted: true, denial: None, transport: "streamable_http" } }
    fn deny(denial: HttpDenialCode) -> Self { Self { accepted: false, denial: Some(denial), transport: "streamable_http" } }
}

/// Admits one optional HTTP request without retaining request content or secrets.
pub fn admit(policy: &HttpAdmissionPolicy, request: &HttpAdmissionRequest<'_>) -> HttpAdmissionReceipt {
    if !request.peer_ip.is_loopback() { return HttpAdmissionReceipt::deny(HttpDenialCode::NonLoopbackPeer); }
    // Host resolution must be loopback too: a hostname changing between checks
    // is denied rather than being trusted after a DNS rebinding attempt.
    if !request.resolved_host_ip.is_loopback() { return HttpAdmissionReceipt::deny(HttpDenialCode::DnsRebinding); }
    if !policy.allowed_hosts.iter().any(|value| value == request.host) { return HttpAdmissionReceipt::deny(HttpDenialCode::HostNotAllowed); }
    if !policy.allowed_origins.iter().any(|value| value == request.origin) { return HttpAdmissionReceipt::deny(HttpDenialCode::OriginNotAllowed); }
    if policy.installation_id != request.installation_id { return HttpAdmissionReceipt::deny(HttpDenialCode::InstallationMismatch); }
    if request.bearer_token.is_empty() { return HttpAdmissionReceipt::deny(HttpDenialCode::MissingBearer); }
    if request.bearer_token.len() > 512 || !constant_time_eq(request.bearer_token.as_bytes(), policy.expected_bearer_token.as_bytes()) { return HttpAdmissionReceipt::deny(HttpDenialCode::InvalidBearer); }
    if request.session_binding.is_empty() || request.session_binding.len() > 256 || !constant_time_eq(request.session_binding.as_bytes(), policy.expected_session_binding.as_bytes()) { return HttpAdmissionReceipt::deny(HttpDenialCode::SessionMismatch); }
    if request.body_bytes > policy.max_body_bytes { return HttpAdmissionReceipt::deny(HttpDenialCode::BodyTooLarge); }
    if request.deadline_ms == 0 || request.deadline_ms > policy.max_deadline_ms { return HttpAdmissionReceipt::deny(HttpDenialCode::DeadlineTooLong); }
    HttpAdmissionReceipt::allow()
}

fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    if left.len() != right.len() { return false; }
    left.iter().zip(right).fold(0_u8, |difference, (a, b)| difference | (a ^ b)) == 0
}
