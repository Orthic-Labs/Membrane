//! Crypt durable store implementation.
//!
//! These modules retain existing SQLite schema, durable identifiers, and installation lineage.

pub mod context_telemetry;
pub mod db;
pub mod installation_identity;
pub mod maintenance_exec;
pub mod memdb;
pub mod scope;
pub mod temporal;
pub mod time;

pub use maintenance_exec::{
    BoundedMaintenanceOperation, MaintenanceExecError, MaintenanceExecKind,
    MaintenanceExecOutcome, MaintenanceExecReceipt, MaintenanceUnitOfWork,
    MAINTENANCE_EXEC_RECEIPT_SCHEMA_VERSION,
};
pub use memdb::MemDb;
pub use temporal::{
    TemporalFact, TemporalFactQuery, TemporalFactReceipt, TemporalFactStore, TemporalTransition,
};
