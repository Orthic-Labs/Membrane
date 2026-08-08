//! MBR-402 bounded batch admission constants and typed terminal receipts.
use serde::{Deserialize, Serialize};
pub const MAX_ITEMS: usize = 50;
pub const MAX_BYTES: usize = 1024 * 1024;
pub const MAX_TOKENS: usize = 50_000;
pub const MAX_DEADLINE_MS: u64 = 5_000;
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TerminalReceiptV1 {
    pub id: String,
    pub status: String,
    #[serde(default)]
    pub code: Option<String>,
    #[serde(default)]
    pub data: Option<serde_json::Value>,
}
