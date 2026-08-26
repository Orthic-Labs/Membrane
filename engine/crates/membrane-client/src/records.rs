//! Wire-compatible records used by resident memory operations.

use crate::error::ClientError;
use serde_json::{Map, Value};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemoryTier {
    Working,
    Episodic,
    Semantic,
}

impl MemoryTier {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Working => "Working",
            Self::Episodic => "Episodic",
            Self::Semantic => "Semantic",
        }
    }
    pub fn parse(value: &str) -> Option<Self> {
        match value.to_ascii_lowercase().as_str() {
            "working" => Some(Self::Working),
            "episodic" => Some(Self::Episodic),
            "semantic" => Some(Self::Semantic),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct MemoryEntry {
    pub id: String,
    pub tier: MemoryTier,
    pub content: String,
    pub keywords: Vec<String>,
    pub score: f64,
    pub created_at: String,
    pub access_count: u32,
    pub embedding: Option<Vec<f32>>,
    pub scope_id: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct FullRecord {
    pub id: String,
    pub content: String,
    pub access_count: u32,
    pub lifecycle: Option<Value>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemoryListRow {
    pub id: String,
    pub tier: MemoryTier,
    pub chars: usize,
    pub access_count: u32,
    pub inject_count: u32,
}

fn required_string(row: &Map<String, Value>, key: &str) -> Result<String, ClientError> {
    row.get(key)
        .and_then(Value::as_str)
        .filter(|v| !v.is_empty())
        .map(str::to_string)
        .ok_or_else(|| ClientError::Protocol {
            code: "record_malformed".into(),
            message: format!("record field {key} is missing"),
            details: Map::new(),
        })
}

pub(crate) fn entry(value: &Value) -> Result<MemoryEntry, ClientError> {
    let row = value.as_object().ok_or_else(|| {
        ClientError::protocol("record_malformed", "memory entry is not an object")
    })?;
    let tier_text = required_string(row, "tier")?;
    let tier = MemoryTier::parse(&tier_text)
        .ok_or_else(|| ClientError::protocol("record_malformed", "unknown memory tier"))?;
    let keywords = row
        .get("keywords")
        .and_then(Value::as_array)
        .unwrap_or(&Vec::new())
        .iter()
        .filter_map(Value::as_str)
        .map(str::to_string)
        .collect();
    let embedding = row.get("embedding").and_then(Value::as_array).map(|items| {
        items
            .iter()
            .filter_map(Value::as_f64)
            .map(|v| v as f32)
            .collect()
    });
    Ok(MemoryEntry {
        id: required_string(row, "id")?,
        tier,
        content: required_string(row, "content")?,
        keywords,
        score: row.get("score").and_then(Value::as_f64).unwrap_or(0.0),
        created_at: row
            .get("created_at")
            .or_else(|| row.get("createdAt"))
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string(),
        access_count: row
            .get("access_count")
            .or_else(|| row.get("accessCount"))
            .and_then(Value::as_u64)
            .unwrap_or(0) as u32,
        embedding,
        scope_id: row
            .get("scope")
            .or_else(|| row.get("scope_id"))
            .or_else(|| row.get("scopeId"))
            .and_then(Value::as_str)
            .unwrap_or("global")
            .to_string(),
    })
}

pub(crate) fn full(value: &Value) -> Result<FullRecord, ClientError> {
    let row = value
        .as_object()
        .ok_or_else(|| ClientError::protocol("record_malformed", "full record is not an object"))?;
    Ok(FullRecord {
        id: required_string(row, "id")?,
        content: required_string(row, "content")?,
        access_count: row
            .get("access_count")
            .or_else(|| row.get("accessCount"))
            .and_then(Value::as_u64)
            .unwrap_or(0) as u32,
        lifecycle: row.get("lifecycle").cloned(),
    })
}

pub(crate) fn list_row(value: &Value) -> Result<MemoryListRow, ClientError> {
    let row = value.as_object().ok_or_else(|| {
        ClientError::protocol("record_malformed", "memory list row is not an object")
    })?;
    let tier = row
        .get("tier")
        .and_then(Value::as_str)
        .and_then(MemoryTier::parse)
        .ok_or_else(|| {
            ClientError::protocol("record_malformed", "memory list row has unknown tier")
        })?;
    Ok(MemoryListRow {
        id: required_string(row, "id")?,
        tier,
        chars: row.get("chars").and_then(Value::as_u64).ok_or_else(|| {
            ClientError::protocol("record_malformed", "memory list row has no character count")
        })? as usize,
        access_count: row
            .get("access")
            .or_else(|| row.get("access_count"))
            .and_then(Value::as_u64)
            .unwrap_or(0) as u32,
        inject_count: row
            .get("inject")
            .or_else(|| row.get("inject_count"))
            .and_then(Value::as_u64)
            .unwrap_or(0) as u32,
    })
}
