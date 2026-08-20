//! MBR-508: crash-safe, bounded execution of previously-planned Cortex maintenance.
//!
//! Membrane lifecycle decides *whether* a maintenance request may run: it
//! checks a caller-supplied authority receipt through a trusted verifier, refuses
//! self-certification, and bounds budget and deadline. It never opens storage. This module is
//! the store-side complement: it executes an already-planned, already-bounded operation
//! (consolidation, contradiction detection, index maintenance, or proposal creation) against the
//! Cortex SQLite store, and nothing else.
//!
//! [`BoundedMaintenanceOperation`] mirrors Membrane's bounded maintenance operation
//! by contract, not by crate dependency: `cortex-store` is already a dependency of
//! `membrane-runtime`, so a `cortex-store` -> resident edge would create a cycle. A host that already holds a planned
//! operation copies its five fields into a `BoundedMaintenanceOperation` before calling
//! [`MemDb::execute_bounded_maintenance`].
//!
//! ## Guarantees this module makes
//!
//! - **Cannot self-certify authority.** This module grants nothing and verifies nothing; it only
//!   refuses. Without a non-empty `authority_receipt_id` it returns
//!   `MaintenanceExecError::MissingAuthority` before opening a transaction. The receipt_id itself
//!   was only ever produced by the upstream planner's own authority check -- this module trusts
//!   that but performs no verification of its own, so there is no code path here that can mint or
//!   confirm authority.
//! - **Crash-safe: the store is never observed partially written.** Every unit the job offers
//!   runs inside one `IMMEDIATE` SQLite transaction. The transaction is committed only when every
//!   offered unit completed within budget and deadline without cancellation; any other outcome
//!   drops the transaction uncommitted, which SQLite rolls back -- including after a hard process
//!   crash, because an uncommitted transaction's frames are never checkpointed into the main
//!   database file. A bounded maintenance invocation is therefore all-or-nothing: a job that does
//!   not fit inside its budget or deadline leaves the store exactly as it found it and can be
//!   retried later as a fresh, smaller request.
//! - **Fully cancellable.** `is_cancelled` is polled before every unit; a `true` result rolls back
//!   the whole invocation immediately.
//! - **Auditable.** Every invocation that gets past the authority check returns a content-free
//!   [`MaintenanceExecReceipt`] recording what was requested, what happened, and why. It never
//!   carries document text, embeddings, paths, or maintenance results.

use crate::memdb::MemDb;
use rusqlite::{Transaction, TransactionBehavior};
use serde::{Deserialize, Serialize};

pub const MAINTENANCE_EXEC_RECEIPT_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MaintenanceExecKind {
    Consolidation,
    ContradictionDetection,
    IndexMaintenance,
    ProposalCreation,
}

/// A previously planned, budget/deadline-bounded unit of work. Construct this only from a
/// Membrane maintenance operation that a trusted planner already
/// returned; this type carries no verification logic of its own and none is added later.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BoundedMaintenanceOperation {
    pub request_id: String,
    pub scheduler_id: String,
    pub kind: MaintenanceExecKind,
    pub budget_units: u32,
    pub deadline_unix_ms: u64,
    pub authority_receipt_id: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MaintenanceExecOutcome {
    Committed,
    RolledBackDeadlineExpired,
    RolledBackCancelled,
    RolledBackBudgetExhausted,
    RolledBackUnitFailed,
}

/// Content-free execution receipt. Never carries document text, embeddings, paths, or results.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MaintenanceExecReceipt {
    pub schema_version: u32,
    pub request_id: String,
    pub scheduler_id: String,
    pub kind: MaintenanceExecKind,
    pub authority_receipt_id: String,
    pub outcome: MaintenanceExecOutcome,
    pub units_offered: u32,
    pub units_committed: u32,
    pub started_unix_ms: u64,
    pub finished_unix_ms: u64,
}

#[derive(Debug, thiserror::Error)]
pub enum MaintenanceExecError {
    #[error("bounded maintenance operation carries no authority_receipt_id; refusing to run")]
    MissingAuthority,
    #[error("bounded maintenance execution: {0}")]
    Sqlite(#[from] rusqlite::Error),
}

/// One discrete, individually-bounded step of a maintenance job (one document consolidated, one
/// contradiction pair checked, one index entry rebuilt, one proposal drafted, ...). The executor
/// -- never the implementation -- owns transaction lifetime: `run_unit` must not commit or roll
/// back the transaction it is given.
pub trait MaintenanceUnitOfWork {
    /// Total number of units this job offers in this invocation.
    fn unit_count(&self) -> u32;
    /// Perform unit `unit_index` inside the open transaction. `Ok(units_spent)` reports how many
    /// budget units it consumed (at least 1 is charged regardless of the returned value); `Err`
    /// aborts the whole invocation and rolls back every unit applied so far.
    fn run_unit(&mut self, tx: &Transaction<'_>, unit_index: u32) -> Result<u32, String>;
}

impl MemDb {
    /// Execute a bounded maintenance operation against the Cortex store. See the module docs for
    /// the crash-safety, cancellation, and authority guarantees this makes.
    pub fn execute_bounded_maintenance<W: MaintenanceUnitOfWork>(
        &self,
        operation: &BoundedMaintenanceOperation,
        now_unix_ms: &dyn Fn() -> u64,
        is_cancelled: &dyn Fn() -> bool,
        work: &mut W,
    ) -> Result<MaintenanceExecReceipt, MaintenanceExecError> {
        if operation.authority_receipt_id.trim().is_empty() {
            return Err(MaintenanceExecError::MissingAuthority);
        }

        let started = now_unix_ms();
        let units_offered = work.unit_count();

        if operation.budget_units == 0 {
            return Ok(finished_receipt(
                operation,
                MaintenanceExecOutcome::RolledBackBudgetExhausted,
                units_offered,
                0,
                started,
                started,
            ));
        }
        if started >= operation.deadline_unix_ms {
            return Ok(finished_receipt(
                operation,
                MaintenanceExecOutcome::RolledBackDeadlineExpired,
                units_offered,
                0,
                started,
                started,
            ));
        }

        let mut conn = self.lock();
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;

        let mut units_committed = 0u32;
        let mut outcome = MaintenanceExecOutcome::Committed;
        for unit_index in 0..units_offered {
            if is_cancelled() {
                outcome = MaintenanceExecOutcome::RolledBackCancelled;
                break;
            }
            if now_unix_ms() >= operation.deadline_unix_ms {
                outcome = MaintenanceExecOutcome::RolledBackDeadlineExpired;
                break;
            }
            if units_committed >= operation.budget_units {
                outcome = MaintenanceExecOutcome::RolledBackBudgetExhausted;
                break;
            }
            match work.run_unit(&tx, unit_index) {
                Ok(spent) => units_committed = units_committed.saturating_add(spent.max(1)),
                Err(_) => {
                    outcome = MaintenanceExecOutcome::RolledBackUnitFailed;
                    break;
                }
            }
        }

        let finished = now_unix_ms();
        match outcome {
            // Only a full, in-budget, on-time, uncancelled pass over every offered unit commits.
            MaintenanceExecOutcome::Committed => tx.commit()?,
            // Every other outcome drops the transaction uncommitted; SQLite rolls it back, so
            // nothing this invocation attempted is ever observable -- including across a crash.
            _ => drop(tx),
        }

        Ok(finished_receipt(
            operation,
            outcome,
            units_offered,
            if outcome == MaintenanceExecOutcome::Committed {
                units_committed
            } else {
                0
            },
            started,
            finished,
        ))
    }
}

fn finished_receipt(
    operation: &BoundedMaintenanceOperation,
    outcome: MaintenanceExecOutcome,
    units_offered: u32,
    units_committed: u32,
    started_unix_ms: u64,
    finished_unix_ms: u64,
) -> MaintenanceExecReceipt {
    MaintenanceExecReceipt {
        schema_version: MAINTENANCE_EXEC_RECEIPT_SCHEMA_VERSION,
        request_id: operation.request_id.clone(),
        scheduler_id: operation.scheduler_id.clone(),
        kind: operation.kind,
        authority_receipt_id: operation.authority_receipt_id.clone(),
        outcome,
        units_offered,
        units_committed,
        started_unix_ms,
        finished_unix_ms,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    fn sample_operation(
        budget_units: u32,
        deadline_unix_ms: u64,
        authority_receipt_id: &str,
    ) -> BoundedMaintenanceOperation {
        BoundedMaintenanceOperation {
            request_id: "req-exec-1".into(),
            scheduler_id: "scheduler-1".into(),
            kind: MaintenanceExecKind::IndexMaintenance,
            budget_units,
            deadline_unix_ms,
            authority_receipt_id: authority_receipt_id.into(),
        }
    }

    fn ensure_scratch_table(db: &MemDb) {
        db.lock()
            .execute_batch(
                "CREATE TABLE IF NOT EXISTS mbr508_test_scratch (
                    id INTEGER PRIMARY KEY,
                    unit_index INTEGER NOT NULL
                )",
            )
            .expect("create scratch table");
    }

    fn scratch_row_count(db: &MemDb) -> i64 {
        db.lock()
            .query_row("SELECT COUNT(*) FROM mbr508_test_scratch", [], |row| {
                row.get(0)
            })
            .expect("count scratch rows")
    }

    struct RecordingWork {
        units: u32,
        fail_at: Option<u32>,
        calls: Cell<u32>,
    }

    impl RecordingWork {
        fn new(units: u32) -> Self {
            Self {
                units,
                fail_at: None,
                calls: Cell::new(0),
            }
        }
    }

    impl MaintenanceUnitOfWork for RecordingWork {
        fn unit_count(&self) -> u32 {
            self.units
        }

        fn run_unit(&mut self, tx: &Transaction<'_>, unit_index: u32) -> Result<u32, String> {
            self.calls.set(self.calls.get() + 1);
            if self.fail_at == Some(unit_index) {
                return Err("synthetic unit failure".into());
            }
            tx.execute(
                "INSERT INTO mbr508_test_scratch (unit_index) VALUES (?1)",
                [unit_index],
            )
            .map_err(|error| error.to_string())?;
            Ok(1)
        }
    }

    #[test]
    fn refuses_to_run_without_an_authority_receipt_id() {
        let db = MemDb::open_in_memory();
        ensure_scratch_table(&db);
        let operation = sample_operation(10, 10_000, "");
        let mut work = RecordingWork::new(3);

        let error = db
            .execute_bounded_maintenance(&operation, &|| 0, &|| false, &mut work)
            .expect_err("empty authority must be refused");

        assert!(matches!(error, MaintenanceExecError::MissingAuthority));
        assert_eq!(work.calls.get(), 0, "no unit should run without authority");
        assert_eq!(scratch_row_count(&db), 0);
    }

    #[test]
    fn commits_every_unit_when_within_budget_and_deadline() {
        let db = MemDb::open_in_memory();
        ensure_scratch_table(&db);
        let operation = sample_operation(10, 10_000, "auth-exec-1");
        let mut work = RecordingWork::new(3);

        let receipt = db
            .execute_bounded_maintenance(&operation, &|| 0, &|| false, &mut work)
            .expect("well-formed run succeeds");

        assert_eq!(receipt.outcome, MaintenanceExecOutcome::Committed);
        assert_eq!(receipt.units_offered, 3);
        assert_eq!(receipt.units_committed, 3);
        assert_eq!(receipt.authority_receipt_id, "auth-exec-1");
        assert_eq!(scratch_row_count(&db), 3);
    }

    #[test]
    fn cancellation_rolls_back_every_unit_this_invocation_applied() {
        let db = MemDb::open_in_memory();
        ensure_scratch_table(&db);
        let operation = sample_operation(10, 10_000, "auth-exec-1");
        let mut work = RecordingWork::new(5);
        let seen = Cell::new(0u32);
        let is_cancelled = || {
            let count = seen.get();
            seen.set(count + 1);
            count >= 2 // cancel once two units have already been checked & applied
        };

        let receipt = db
            .execute_bounded_maintenance(&operation, &|| 0, &is_cancelled, &mut work)
            .expect("cancellation is a normal, receipted outcome");

        assert_eq!(receipt.outcome, MaintenanceExecOutcome::RolledBackCancelled);
        assert_eq!(
            receipt.units_committed, 0,
            "a rolled-back run commits nothing"
        );
        assert_eq!(
            scratch_row_count(&db),
            0,
            "cancellation must leave zero partial rows"
        );
    }

    #[test]
    fn deadline_already_passed_never_touches_the_store() {
        let db = MemDb::open_in_memory();
        ensure_scratch_table(&db);
        let operation = sample_operation(10, 1_000, "auth-exec-1");
        let mut work = RecordingWork::new(3);

        let receipt = db
            .execute_bounded_maintenance(&operation, &|| 1_000, &|| false, &mut work)
            .expect("expired deadline is a normal, receipted outcome");

        assert_eq!(
            receipt.outcome,
            MaintenanceExecOutcome::RolledBackDeadlineExpired
        );
        assert_eq!(
            work.calls.get(),
            0,
            "no unit should run once the deadline has passed"
        );
        assert_eq!(scratch_row_count(&db), 0);
    }

    #[test]
    fn budget_exhaustion_rolls_back_the_units_already_applied_this_invocation() {
        let db = MemDb::open_in_memory();
        ensure_scratch_table(&db);
        let operation = sample_operation(2, 10_000, "auth-exec-1");
        let mut work = RecordingWork::new(5);

        let receipt = db
            .execute_bounded_maintenance(&operation, &|| 0, &|| false, &mut work)
            .expect("budget exhaustion is a normal, receipted outcome");

        assert_eq!(
            receipt.outcome,
            MaintenanceExecOutcome::RolledBackBudgetExhausted
        );
        assert_eq!(
            receipt.units_committed, 0,
            "an all-or-nothing invocation never partially persists"
        );
        assert_eq!(
            scratch_row_count(&db),
            0,
            "budget exhaustion must leave zero partial rows"
        );
    }

    #[test]
    fn a_failing_unit_rolls_back_units_that_already_succeeded() {
        let db = MemDb::open_in_memory();
        ensure_scratch_table(&db);
        let operation = sample_operation(10, 10_000, "auth-exec-1");
        let mut work = RecordingWork {
            fail_at: Some(1),
            ..RecordingWork::new(3)
        };

        let receipt = db
            .execute_bounded_maintenance(&operation, &|| 0, &|| false, &mut work)
            .expect("a failing unit is a normal, receipted outcome");

        assert_eq!(
            receipt.outcome,
            MaintenanceExecOutcome::RolledBackUnitFailed
        );
        assert_eq!(
            scratch_row_count(&db),
            0,
            "unit 0's insert must not survive unit 1's failure"
        );
    }

    #[test]
    fn zero_budget_is_rejected_before_touching_the_store() {
        let db = MemDb::open_in_memory();
        ensure_scratch_table(&db);
        let operation = sample_operation(0, 10_000, "auth-exec-1");
        let mut work = RecordingWork::new(3);

        let receipt = db
            .execute_bounded_maintenance(&operation, &|| 0, &|| false, &mut work)
            .expect("zero budget is a normal, receipted outcome");

        assert_eq!(
            receipt.outcome,
            MaintenanceExecOutcome::RolledBackBudgetExhausted
        );
        assert_eq!(work.calls.get(), 0);
        assert_eq!(scratch_row_count(&db), 0);
    }
}
