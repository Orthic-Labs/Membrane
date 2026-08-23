//! Cortex durable store implementation.
//!
//! These modules retain existing SQLite schema, durable identifiers, and installation lineage.
//!
//! This crate's explicit re-exports are its stable storage API. Crate semver
//! and typed record schema versions provide compatibility; SQLite tables and
//! connection details remain implementation-owned.

pub mod absorbed_migrations;
pub mod absorbed_records;
pub mod context_telemetry;
pub mod db;
pub mod fts5;
pub mod installation_identity;
pub mod maintenance_exec;
pub mod memdb;
pub mod scope;
pub mod team_sync;
pub mod temporal;
pub mod time;
pub mod transcript;

pub use absorbed_records::{
    AbsorbedStore, AbsorbedStoreError, AppendOutcome, ArtifactRecord, EventCursor, ProvenanceRef,
    SessionEvent, SessionRecord, TaskRecord, ABSORBED_SCHEMA_VERSION,
};
pub use fts5::{
    sanitize_match_query, Fts5Document, Fts5Error, Fts5Hit, Fts5Projection, ProjectionState,
    FTS5_TABLE,
};

pub use maintenance_exec::{
    BoundedMaintenanceOperation, MaintenanceExecError, MaintenanceExecKind, MaintenanceExecOutcome,
    MaintenanceExecReceipt, MaintenanceUnitOfWork, MAINTENANCE_EXEC_RECEIPT_SCHEMA_VERSION,
};
pub use memdb::MemDb;
pub use team_sync::{
    TeamSyncAuditRecord, TeamSyncCommitOutcome, TeamSyncCommitReceipt, TeamSyncOptInRecord,
    TeamSyncStoreError,
};
pub use temporal::{
    TemporalFact, TemporalFactQuery, TemporalFactReceipt, TemporalFactStore, TemporalTransition,
};
pub use transcript::{
    TranscriptChunk, TranscriptChunkRecord, TranscriptSearchHit, TranscriptStore,
    TranscriptStoreError, TRANSCRIPT_STORE_SCHEMA_VERSION,
};
