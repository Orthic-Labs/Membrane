use serde::{Deserialize, Serialize};

pub const MAX_BATCH_ITEMS: usize = 50;
pub const MAX_BATCH_BYTES: usize = 1024 * 1024;
pub const MAX_BATCH_TOKENS: usize = 50_000;
pub const MAX_BATCH_DEADLINE_MS: u64 = 5_000;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CodeBatchItemV1 {
    pub id: String,
    pub repository_root: String,
    pub generation_id: String,
    pub source_hash: String,
    pub operation: String,
    #[serde(default)]
    pub node: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CodeBatchReceiptV1 {
    pub id: String,
    pub status: String,
    #[serde(default)]
    pub code: Option<String>,
    #[serde(default)]
    pub data: Option<serde_json::Value>,
}
