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
//! * Taste candidates derive only from caller-selected, external-user
//!   transcript events; exact source digests bind review to those files.
//! * Insights records are diagnostic/reference-only; they never create user
//!   preference authority ([`insights`], [`remediation`]).
//! * Durable outputs cross exactly one typed Cortex admission boundary,
//!   represented here as a proposal envelope ([`gates`]); Adapt never owns a
//!   parallel durable store.
//! * The three admission decisions (proposal eligibility, Cortex durable
//!   admission, Membrane context admission) are distinct types that never
//!   collapse into each other ([`gates`]).

pub mod adaptive;
pub mod admission;
pub mod attribution;
pub mod authority;
pub mod benchmark;
pub mod canonical;
pub mod cli_api;
pub mod context_cost;
pub mod delivery;
pub mod duplicate_groups;
pub mod evidence;
pub mod gates;
pub mod insights;
pub mod lineage;
pub mod manifest;
pub mod model_boundary;
pub mod multiwriter;
pub mod outcomes;
pub mod portable;
pub mod procedural_effectiveness;
pub mod proposal;
pub mod record;
pub mod remediation;
pub mod scope;
pub mod seal;
pub mod taste;
