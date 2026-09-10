//! Native port of `blueprint/src/lib/application/normalize.mjs`.
//!
//! Input normalization for the shared application service. Keeps query
//! adaptation in one place so CLI, MCP, SDK, and hooks all feed the service
//! the same canonical shapes.

use crate::lib_application_errors::BlueprintError;
use serde_json::Value;

pub struct ClampBounds {
    pub min: i64,
    pub max: i64,
    pub fallback: i64,
}

/// Mirrors `clampInt(value, { min, max, fallback })`.
pub fn clamp_int(value: &Value, bounds: &ClampBounds) -> i64 {
    let parsed = match value {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => s.trim().parse::<f64>().ok(),
        Value::Null => None,
        _ => None,
    };
    let Some(parsed) = parsed.filter(|v| v.is_finite()) else {
        return bounds.fallback;
    };
    let truncated = parsed.trunc() as i64;
    truncated.clamp(bounds.min, bounds.max)
}

#[derive(Debug, Clone)]
pub struct NormalizedQuery {
    pub query: String,
    pub limit: i64,
    pub anchors: Vec<String>,
}

#[derive(Debug, Clone, Default)]
pub struct QueryDefaults {
    pub max_limit: Option<i64>,
    pub limit: Option<i64>,
}

/// Mirrors `normalizeQueryInput(input, defaults)`.
pub fn normalize_query_input(input: &Value, defaults: &QueryDefaults) -> Result<NormalizedQuery, BlueprintError> {
    let query = input
        .get("query")
        .and_then(Value::as_str)
        .or_else(|| input.get("task").and_then(Value::as_str))
        .unwrap_or("")
        .trim()
        .to_owned();
    if query.is_empty() {
        return Err(BlueprintError::new("query_required", "A non-empty query is required.", None));
    }
    let limit = clamp_int(
        input.get("limit").unwrap_or(&Value::Null),
        &ClampBounds { min: 1, max: defaults.max_limit.unwrap_or(100), fallback: defaults.limit.unwrap_or(20) },
    );
    let anchors = input
        .get("anchors")
        .and_then(Value::as_array)
        .map(|arr| arr.iter().map(|v| v.as_str().map(str::to_owned).unwrap_or_else(|| v.to_string())).collect())
        .unwrap_or_default();
    Ok(NormalizedQuery { query, limit, anchors })
}

#[derive(Debug, Clone)]
pub struct NormalizedAnchor {
    pub anchor: String,
    pub depth: i64,
    pub budget: i64,
    pub direction: String,
    pub cursor: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct AnchorDefaults {
    pub max_depth: Option<i64>,
    pub depth: Option<i64>,
    pub max_budget: Option<i64>,
    pub budget: Option<i64>,
}

/// Mirrors `normalizeAnchorInput(input, defaults)`.
pub fn normalize_anchor_input(input: &Value, defaults: &AnchorDefaults) -> Result<NormalizedAnchor, BlueprintError> {
    let anchor = input.get("anchor").and_then(Value::as_str).unwrap_or("").trim().to_owned();
    if anchor.is_empty() {
        return Err(BlueprintError::new("anchor_required", "An anchor is required.", None));
    }
    let depth = clamp_int(
        input.get("depth").unwrap_or(&Value::Null),
        &ClampBounds { min: 1, max: defaults.max_depth.unwrap_or(8), fallback: defaults.depth.unwrap_or(1) },
    );
    let budget = clamp_int(
        input.get("budget").unwrap_or(&Value::Null),
        &ClampBounds { min: 128, max: defaults.max_budget.unwrap_or(32000), fallback: defaults.budget.unwrap_or(2000) },
    );
    let direction = input.get("direction").and_then(Value::as_str).unwrap_or("both").to_owned();
    let cursor = input.get("cursor").and_then(Value::as_str).map(str::to_owned);
    Ok(NormalizedAnchor { anchor, depth, budget, direction, cursor })
}
