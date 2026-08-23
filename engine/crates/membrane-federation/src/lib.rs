//! Native, in-process federation for Membrane Pull.
//!
//! Module declarations are intentionally complete here.  Later migration
//! packets add implementations without editing this composition boundary.

pub mod request;
pub mod root;
pub mod release;
pub mod scope;
pub mod freshness;
pub mod deadline;
pub mod scheduler;
pub mod omission;
pub mod normalize;
pub mod merge;
pub mod engine;
pub mod blueprint_client;
pub mod migrate_decisions;
pub mod shadow;
pub mod config;
pub mod error;
pub mod registry;

pub mod providers {
    pub mod anchors;
    pub mod blueprint;
    pub mod rules;
    pub mod live_files;
    pub mod git;
    pub mod audit;
    pub mod architect;
    pub mod skills;
    pub mod cortex;
}

pub use config::{FederationConfig, ProviderConfig, FEDERATION_CONFIG_SCHEMA_VERSION};
pub use engine::FederationEngine;
pub use error::{ConfigError, RegistryError};
pub use registry::ProviderRegistry;
