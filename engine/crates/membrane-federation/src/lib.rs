//! Native, in-process federation for Membrane Pull.
//!
//! Module declarations are intentionally complete here.  Later migration
//! packets add implementations without editing this composition boundary.

pub mod blueprint_client;
pub mod config;
pub mod corrective;
pub mod deadline;
mod egress_redaction;
pub mod engine;
pub mod error;
pub mod freshness;
pub mod merge;
pub mod migrate_decisions;
pub mod normalize;
pub mod omission;
pub mod registry;
pub mod release;
pub mod request;
pub mod requirements;
pub mod root;
pub mod scheduler;
pub mod scope;
pub mod shadow;

pub mod providers {
    pub mod anchors;
    pub mod architect;
    pub mod audit;
    pub mod blueprint;
    pub mod cortex;
    pub mod git;
    pub mod live_files;
    pub mod rules;
    pub mod skills;
}

pub use config::{FederationConfig, ProviderConfig, FEDERATION_CONFIG_SCHEMA_VERSION};
pub use engine::FederationEngine;
pub use error::{ConfigError, RegistryError};
pub use merge::FusionStrategy;
pub use registry::ProviderRegistry;
pub use requirements::{
    AcquisitionPlanV1, CandidateJourneyV1, EvidenceRequirementSetV1,
    RequirementEvidenceMapV1, RequirementFactV1,
};
pub mod lib_federation_index;

pub use lib_federation_index::{
    compose_federated_slices, define_federation_group, is_repo_allowed,
    route_federated_query, route_native_federated_query, NativeBlueprintFederator,
};
