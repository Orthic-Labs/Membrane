//! # membrane-adapt — native Membrane Adapt subsystem
//!
//! Governed behavioral learning: **Taste** (user-backed preference learning)
//! and **Insights** (failure/gotcha learning), implemented as a deterministic,
//! native library with no Python, no Node, no network, and no `unsafe`.
//!
//! Authority boundaries enforced structurally in this crate:
//!
//! * Model output is an untrusted [`model_boundary`] proposal; it can never
//!   set authority class, source identity, signal strength, or permissions.
//! * Taste authority derives only from qualifying user evidence
//!   ([`evidence::UserActEvidenceV1`]).
//! * Insights records are diagnostic/reference-only; they never create user
//!   preference authority ([`insights`], [`remediation`]).
//! * Durable outputs cross exactly one typed Cortex admission boundary,
//!   represented here as a proposal envelope ([`gates`]); Adapt never owns a
//!   parallel durable store.
//! * The three admission decisions (proposal eligibility, Cortex durable
//!   admission, Membrane context admission) are distinct types that never
//!   collapse into each other ([`gates`]).

pub mod canonical;
pub mod evidence;
pub mod authority;
pub mod adaptive;
pub mod scope;
pub mod record;
pub mod seal;
pub mod taste;
pub mod manifest;
pub mod admission;
pub mod model_boundary;
pub mod gates;
pub mod insights;
pub mod remediation;
pub mod outcomes;
pub mod context_cost;
pub mod delivery;
pub mod duplicate_groups;
pub mod multiwriter;
pub mod portable;
pub mod proposal;
pub mod benchmark;
pub mod cli_api;
