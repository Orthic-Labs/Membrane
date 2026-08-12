//! # membrane-supervisor
//!
//! The per-user Membrane supervisor. A separate binary from `membrane` itself; it owns one
//! resident (the `membrane supervisor-child` process), publishes a discovery file for client
//! adapters to read, enforces a single-instance lock, and dedupes the cortex watcher.
//!
//! Why a separate process: the supervisor must outlive a resident crash and respawn it
//! without holding the engine database open. The Hub starts it as a child process,
//! separate from the resident's loopback listener.
//!
//! MBR-201: build a per-user Membrane supervisor.

pub mod admission;
pub mod config;
pub mod error;
pub mod heartbeat;
pub mod lease;
pub mod lock;
pub mod maintenance;
pub mod resident;
pub mod supervisor;
pub mod watcher;

pub use admission::{evaluate, AdmissionDecision};
pub use config::{
    RestartPolicy, SupervisorConfig, WatcherPolicy, CONFIG_SCHEMA_VERSION, DEFAULT_LOOPBACK_PORT,
};
pub use error::{Result, SupervisorError};
pub use heartbeat::{detect_conflation, summarize_status, HeartbeatTable};
pub use lease::{
    publish_lease, SupervisorEndpointV1, SupervisorLeaseV1, SUPERVISOR_ENDPOINT_SCHEMA_VERSION,
    SUPERVISOR_LEASE_SCHEMA_VERSION,
};
pub use lock::{pid_alive_probe, release_if_owned, try_acquire_until, LockOutcome, SupervisorLock};
pub use maintenance::{
    plan as plan_maintenance, AuthorityReceipt, MaintenanceAuthorityVerifier,
    MaintenanceDisposition, MaintenanceKind, MaintenanceOperation, MaintenancePlan,
    MaintenanceReason, MaintenanceReceipt, MaintenanceRequest, MAINTENANCE_RECEIPT_SCHEMA_VERSION,
    MAX_MAINTENANCE_BUDGET_UNITS, MAX_MAINTENANCE_WINDOW_MS,
};
pub use resident::{preflight_resident_binary, ResidentHandle, ResidentInvocation};
pub use supervisor::{
    default_state_path, dry_run, interruptible_sleep, load_or_init_state, save_state, CheckReport,
    CycleOutcome, Supervisor, SupervisorState, SUPERVISOR_STATE_SCHEMA_VERSION,
};
pub use watcher::{
    decide_action, read_watcher_pidfile, two_decisions_agree, WatcherAction, WatcherCoordinator,
};
