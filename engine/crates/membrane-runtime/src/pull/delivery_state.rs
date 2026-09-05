//! Process-local delivered-evidence state for Pull session efficiency.
//!
//! Suppression is conservative: it is enabled only when the consumer
//! explicitly confirms it retains previously delivered evidence. Protected
//! blocks are never suppressed. Unknown host retention therefore preserves
//! current behavior and cannot silently remove the only available proof.

use cortex_core::planner::ContextPacketV1;
use membrane_protocol::digest_str;
use serde::Serialize;
use serde_json::Value;
use std::collections::{BTreeMap, VecDeque};
use std::sync::{Mutex, OnceLock};

const HORIZON_TURNS: u64 = 16;
const MAX_SESSIONS: usize = 256;
const MAX_EVIDENCE_PER_SESSION: usize = 512;

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SuppressionReceiptV1 {
    pub schema_version: u32,
    pub evidence_id: String,
    pub reason: String,
    pub source_hash: String,
    pub last_delivered_turn: u64,
    pub restore_eligibility: Vec<&'static str>,
}

#[derive(Clone, Debug)]
struct DeliveredEvidence {
    identity: String,
    source_hash: String,
    turn: u64,
}

#[derive(Clone, Debug, Default)]
struct SessionState {
    turn: u64,
    delivered: VecDeque<DeliveredEvidence>,
    previous_packet: Option<Value>,
}

#[derive(Default)]
struct DeliveryLedger {
    sessions: BTreeMap<String, SessionState>,
}

static LEDGER: OnceLock<Mutex<DeliveryLedger>> = OnceLock::new();

fn ledger() -> &'static Mutex<DeliveryLedger> {
    LEDGER.get_or_init(|| Mutex::new(DeliveryLedger::default()))
}

fn session_key(repository_id: &str, session_id: &str) -> String {
    digest_str(&format!("pull-session\0{repository_id}\0{session_id}"))
}

fn block_identity(block: &cortex_core::planner::BlockV1) -> String {
    digest_str(&format!(
        "{}\0{}\0{}\0{}",
        block.provider, block.source_ref, block.source_hash, block.id
    ))
}

/// Remove unchanged, non-protected evidence already delivered within the
/// bounded horizon. No suppression occurs unless host retention is explicit.
pub fn suppress_packet(
    packet: &mut ContextPacketV1,
    repository_id: &str,
    session_id: &str,
    host_retains_delivered_evidence: bool,
    explicit_refresh: bool,
) -> Vec<SuppressionReceiptV1> {
    if !host_retains_delivered_evidence || explicit_refresh {
        return Vec::new();
    }
    let key = session_key(repository_id, session_id);
    let Ok(mut guard) = ledger().lock() else {
        return Vec::new();
    };
    let state = guard.sessions.entry(key).or_default();
    state.turn = state.turn.saturating_add(1);
    let current_turn = state.turn;
    state
        .delivered
        .retain(|entry| current_turn.saturating_sub(entry.turn) <= HORIZON_TURNS);

    let mut receipts = Vec::new();
    packet.blocks.retain(|block| {
        if block.protected {
            return true;
        }
        let identity = block_identity(block);
        let prior = state.delivered.iter().rev().find(|entry| {
            entry.identity == identity
                && entry.source_hash == block.source_hash
                && current_turn.saturating_sub(entry.turn) <= HORIZON_TURNS
        });
        if let Some(prior) = prior {
            receipts.push(SuppressionReceiptV1 {
                schema_version: 1,
                evidence_id: block.id.clone(),
                reason: "unchanged_in_horizon".to_owned(),
                source_hash: block.source_hash.clone(),
                last_delivered_turn: prior.turn,
                restore_eligibility: vec![
                    "content_change",
                    "expiry",
                    "unknown_host_state",
                    "explicit_refresh",
                ],
            });
            false
        } else {
            true
        }
    });
    receipts
}

/// Record only blocks that actually left Pull in the selected representation.
pub fn record_selected_packet(
    selected_content: &Value,
    repository_id: &str,
    session_id: &str,
) {
    let Ok(packet) = serde_json::from_value::<ContextPacketV1>(selected_content.clone()) else {
        return;
    };
    let key = session_key(repository_id, session_id);
    let Ok(mut guard) = ledger().lock() else {
        return;
    };
    if guard.sessions.len() >= MAX_SESSIONS && !guard.sessions.contains_key(&key) {
        if let Some(first) = guard.sessions.keys().next().cloned() {
            guard.sessions.remove(&first);
        }
    }
    let state = guard.sessions.entry(key).or_default();
    state.turn = state.turn.saturating_add(1);
    let turn = state.turn;
    for block in &packet.blocks {
        state.delivered.push_back(DeliveredEvidence {
            identity: block_identity(block),
            source_hash: block.source_hash.clone(),
            turn,
        });
    }
    while state.delivered.len() > MAX_EVIDENCE_PER_SESSION {
        state.delivered.pop_front();
    }
    state.previous_packet = Some(selected_content.clone());
}

pub fn previous_packet(repository_id: &str, session_id: &str) -> Option<Value> {
    let key = session_key(repository_id, session_id);
    ledger()
        .lock()
        .ok()
        .and_then(|guard| guard.sessions.get(&key).and_then(|state| state.previous_packet.clone()))
}
