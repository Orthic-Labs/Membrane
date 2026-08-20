//! Signature snapshot for Cortex durable-memory API.

#![allow(dead_code)]

use cortex::{CheckpointSourceRefV1, CheckpointV1, MemDb, MemoryStore};

#[test]
fn cortex_exposes_durable_memory_types_only() {
    let db = MemDb::open_in_memory();
    let _store = MemoryStore::open(db);
    let _: Option<CheckpointV1> = None;
    let _: Option<CheckpointSourceRefV1> = None;
}
