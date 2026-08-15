use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Static, secret-free `orthic.product-manifest.v2` contract.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ManifestV2 {
    pub schema_version: u32,
    pub product_id: String,
    pub display_name: String,
    pub product_version: String,
    pub hub_compat_range: String,
    pub install_root: String,
    pub service_start: Vec<String>,
    pub service_stop: Vec<String>,
    pub icon: String,
    /// Exact released add-on digest. Validator requires this before launch.
    pub artifact_digest: String,
}

/// Source-compatible name retained for product modules migrating to v2.
pub type ManifestV1 = ManifestV2;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Copy)]
#[serde(rename_all = "lowercase")]
pub enum SectionState {
    Available,
    Degraded,
    Unavailable,
}

/// Content-free v2 snapshot bounds mirroring `schema/snapshot.v2.schema.json`.
/// Total payload cap (65536 bytes) is enforced at the Hub read boundary by
/// `hub_runtime::MAX_SNAPSHOT_BYTES`; section count (1–16) and item count
/// (≤1000) are enforced by `hub_runtime::fetch_snapshot`. The closed item
/// field set, primitive-only item values, and per-field string caps are
/// enforced at deserialization by [`deserialize_bounded_items`], which is the
/// only OR-CONTRACTS hook on the production fetch path (it runs inside
/// `hub_runtime`'s `serde_json::from_slice::<SnapshotV2>` via `SectionV1`).
/// Enforced by `hub_runtime::fetch_snapshot` (OR-SUPERVISOR) at the read
/// boundary; documented here as the contract of record.
#[allow(dead_code)]
pub const SNAPSHOT_MAX_SECTIONS: usize = 16;
/// Enforced by `hub_runtime::fetch_snapshot` (OR-SUPERVISOR) at the read
/// boundary; documented here as the contract of record.
#[allow(dead_code)]
pub const SNAPSHOT_MAX_ITEMS_PER_SECTION: usize = 1000;
pub const SNAPSHOT_MAX_ITEM_FIELDS: usize = 8;
pub const SNAPSHOT_MAX_ITEM_STRING_BYTES: usize = 512;
pub const SNAPSHOT_MAX_ITEM_LABEL_BYTES: usize = 128;
pub const SNAPSHOT_MAX_ITEM_KIND_BYTES: usize = 64;
/// Total payload cap; enforced by `hub_runtime::MAX_SNAPSHOT_BYTES` (65536).
/// Documented here so the schema, Rust, and Node agree on one number.
#[allow(dead_code)]
pub const SNAPSHOT_MAX_TOTAL_BYTES: usize = 65_536;

/// The closed set of named evidence-handle fields an item may carry. Mirrors
/// `schema/snapshot.v2.schema.json item.properties`. The contract forbids
/// arbitrary maps: a product may not invent item property names.
pub const ALLOWED_ITEM_FIELDS: &[&str] = &[
    "label",
    "kind",
    "count",
    "severity",
    "evidence",
    "resolver",
    "observedAtUnixMs",
    "stale",
];

/// Deserialize `items` enforcing the closed, bounded, content-free v2
/// contract **per field** at parse time, matching `schema/snapshot.v2.schema.json`
/// `sections.*.items[]` and the Node mirror `schema/validate.mjs` exactly.
///
/// This runs on the production fetch path: `hub_runtime::fetch_snapshot`
/// deserializes a `SnapshotV2` whose `sections` are `BTreeMap<String,
/// SectionV1>`, so every fetched snapshot is bounded here. The field type stays
/// `Vec<serde_json::Value>` so `hub_runtime`'s existing `item.is_object()` /
/// `items.len()` checks — including its certified
/// `fetch_rejects_nonobject_snapshot_items` test (which expects
/// `snapshot_bounds_invalid`) — are unchanged. Non-object items intentionally
/// pass through here so `hub_runtime` returns the certified
/// `snapshot_bounds_invalid`. Object items are content-checked per field:
/// unknown fields, wrong scalar types, `null`/missing required `label`, negative
/// or non-numeric `count`, invalid `severity` enum, fractional/negative
/// `observedAtUnixMs`, non-boolean `stale`, nested object/array values, oversized
/// per-field strings, and too many properties each fail here as a serde error,
/// which `hub_runtime` maps to `snapshot_schema_invalid` (typed, closed). The
/// error code strings mirror `schema/validate.mjs` so the two validators emit a
/// contract-identical typed rejection for the same invalid input.
fn deserialize_bounded_items<'de, D>(
    deserializer: D,
) -> Result<Option<Vec<serde_json::Value>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let opt: Option<Vec<serde_json::Value>> = Option::<Vec<serde_json::Value>>::deserialize(deserializer)?;
    match opt {
        None => Ok(None),
        Some(items) => {
            for item in &items {
                // Bare-primitive (non-object) items intentionally pass through;
                // `hub_runtime`'s `!item.is_object()` check returns the certified
                // `snapshot_bounds_invalid`, which this deserializer must not
                // preempt (its own certified test).
                if let Some(map) = item.as_object() {
                    // Closed field set first: arbitrary maps are forbidden.
                    for key in map.keys() {
                        if !ALLOWED_ITEM_FIELDS.contains(&key.as_str()) {
                            return Err(serde::de::Error::custom("snapshot_item_field_unknown"));
                        }
                    }
                    // `label` is required, string, 1..=128.
                    match map.get("label") {
                        Some(serde_json::Value::String(s)) if !s.is_empty()
                            && s.len() <= SNAPSHOT_MAX_ITEM_LABEL_BYTES => {}
                        _ => return Err(serde::de::Error::custom("snapshot_item_string_too_long")),
                    }
                    // Per named optional field: exact scalar type & bounds.
                    for (key, value) in map {
                        match key.as_str() {
                            // `kind`: optional string ≤ 64.
                            "kind" => match value.as_str() {
                                Some(s) if s.len() <= SNAPSHOT_MAX_ITEM_KIND_BYTES => {}
                                _ => return Err(serde::de::Error::custom("snapshot_item_string_too_long")),
                            },
                            // `count`: optional number ≥ 0 (integer or float).
                            "count" => match value.as_f64() {
                                Some(n) if n >= 0.0 => {}
                                _ => return Err(serde::de::Error::custom("snapshot_item_value_not_primitive")),
                            },
                            // `severity`: optional enum.
                            "severity" => match value.as_str() {
                                Some("info") | Some("warning") | Some("error") | Some("critical") => {}
                                _ => return Err(serde::de::Error::custom("snapshot_item_value_not_primitive")),
                            },
                            // `evidence` / `resolver`: optional string ≤ 512.
                            "evidence" | "resolver" => match value.as_str() {
                                Some(s) if s.len() <= SNAPSHOT_MAX_ITEM_STRING_BYTES => {}
                                _ => return Err(serde::de::Error::custom("snapshot_item_string_too_long")),
                            },
                            // `observedAtUnixMs`: optional non-negative integer.
                            "observedAtUnixMs" => match value.as_u64() {
                                Some(_) => {}
                                None => match value.as_f64() {
                                    // Tolerate an integral float (e.g. 1.0) the way
                                    // JS `Number.isInteger` does; reject fractional / negative.
                                    Some(f) if f.fract() == 0.0 && f >= 0.0 => {}
                                    _ => return Err(serde::de::Error::custom("snapshot_item_value_not_primitive")),
                                },
                            },
                            // `stale`: optional boolean.
                            "stale" => match value.as_bool() {
                                Some(_) => {}
                                None => return Err(serde::de::Error::custom("snapshot_item_value_not_primitive")),
                            },
                            // `label` already checked above.
                            "label" => {}
                            // Unreachable: the closed-set check above rejects first.
                            _ => return Err(serde::de::Error::custom("snapshot_item_field_unknown")),
                        }
                    }
                    // Defence in depth: the closed 8-name set makes the cap
                    // unreachable with all-allowed names, but a future schema
                    // widening must still respect the bound.
                    if map.len() > SNAPSHOT_MAX_ITEM_FIELDS {
                        return Err(serde::de::Error::custom("snapshot_item_properties_out_of_bounds"));
                    }
                }
            }
            Ok(Some(items))
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SectionV1 {
    pub state: SectionState,
    pub reason: String,
    #[serde(default, deserialize_with = "deserialize_bounded_items")]
    pub items: Option<Vec<serde_json::Value>>,
    pub evidence: Option<String>,
    pub resolver: Option<String>,
    pub observed_at_unix_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SnapshotV2 {
    pub schema_version: u32,
    pub product_id: String,
    pub observed_at_unix_ms: u64,
    pub sections: HashMap<String, SectionV1>,
    pub stale: Option<bool>,
    pub cache_age_ms: Option<u64>,
}

pub type SnapshotV1 = SnapshotV2;

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn snapshot_round_trip() {
        let json = r#"{"schemaVersion":2,"productId":"membrane","observedAtUnixMs":123,"sections":{"deliveries":{"state":"available","reason":"ok"}}}"#;
        let snap: SnapshotV1 = serde_json::from_str(json).unwrap();
        assert_eq!(snap.schema_version, 2);
        assert_eq!(snap.sections["deliveries"].state, SectionState::Available);
        let ser = serde_json::to_string(&snap).unwrap();
        let de: SnapshotV1 = serde_json::from_str(&ser).unwrap();
        assert_eq!(snap, de);
    }
    #[test]
    fn manifest_round_trip() {
        let json = r#"{"schemaVersion":2,"productId":"cortex","displayName":"Cortex","productVersion":"1.0.0","hubCompatRange":">=0.1.0","installRoot":"/tmp/cortex","serviceStart":["/tmp/cortex/bin"],"serviceStop":[],"icon":"/tmp/cortex/icon.png","artifactDigest":"sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"}"#;
        let m: ManifestV1 = serde_json::from_str(json).unwrap();
        assert_eq!(m.product_id, "cortex");
        let ser = serde_json::to_string(&m).unwrap();
        let de: ManifestV1 = serde_json::from_str(&ser).unwrap();
        assert_eq!(m, de);
    }
    #[test]
    fn section_state_serde_lowercase() {
        assert_eq!(serde_json::to_string(&SectionState::Available).unwrap(), "\"available\"");
        assert_eq!(serde_json::to_string(&SectionState::Degraded).unwrap(), "\"degraded\"");
        assert_eq!(serde_json::to_string(&SectionState::Unavailable).unwrap(), "\"unavailable\"");
        let s: SectionState = serde_json::from_str("\"degraded\"").unwrap();
        assert_eq!(s, SectionState::Degraded);
    }
    #[test]
    fn section_items_deserialize_enforces_content_free_contract() {
        // A valid closed, bounded, content-free item round-trips.
        let json = r#"{"state":"degraded","reason":"slow","items":[{"label":"d1","kind":"delivery","count":3,"severity":"warning","evidence":"hub.snapshot#1","resolver":"retry","observedAtUnixMs":999,"stale":false}],"evidence":"log line","resolver":"retry","observedAtUnixMs":999}"#;
        let section: SectionV1 = serde_json::from_str(json).unwrap();
        assert_eq!(section.items.as_ref().unwrap().len(), 1);
        let ser = serde_json::to_string(&section).unwrap();
        let de: SectionV1 = serde_json::from_str(&ser).unwrap();
        assert_eq!(serde_json::to_string(&de).unwrap(), ser);

        // Unknown item field is an arbitrary map entry — the contract forbids it.
        let bad = r#"{"state":"degraded","reason":"slow","items":[{"label":"d1","rogueKey":"v"}]}"#;
        let err = serde_json::from_str::<SectionV1>(bad).unwrap_err();
        assert!(err.to_string().contains("snapshot_item_field_unknown"), "{err}");

        // Nested object value on an allowed field is content-bearing and rejected.
        let bad = r#"{"state":"degraded","reason":"slow","items":[{"label":"d1","count":{"a":1}}]}"#;
        let err = serde_json::from_str::<SectionV1>(bad).unwrap_err();
        assert!(err.to_string().contains("snapshot_item_value_not_primitive"), "{err}");

        // Nested array value on an allowed field is content-bearing and rejected.
        let bad = r#"{"state":"degraded","reason":"slow","items":[{"label":"d1","count":[1,2]}]}"#;
        let err = serde_json::from_str::<SectionV1>(bad).unwrap_err();
        assert!(err.to_string().contains("snapshot_item_value_not_primitive"));

        // Oversized label is rejected (per-field cap, not the generic 512).
        let bad = format!(r#"{{"state":"degraded","reason":"slow","items":[{{"label":"{}"}}]}}"#, "x".repeat(SNAPSHOT_MAX_ITEM_LABEL_BYTES + 1));
        let err = serde_json::from_str::<SectionV1>(&bad).unwrap_err();
        assert!(err.to_string().contains("snapshot_item_string_too_long"));

        // Oversized evidence handle is rejected.
        let bad = format!(r#"{{"state":"degraded","reason":"slow","items":[{{"label":"d1","evidence":"{}"}}]}}"#, "x".repeat(SNAPSHOT_MAX_ITEM_STRING_BYTES + 1));
        let err = serde_json::from_str::<SectionV1>(&bad).unwrap_err();
        assert!(err.to_string().contains("snapshot_item_string_too_long"));

        // The closed field set has exactly 8 allowed names, so the per-item
        // maxProperties:8 cap is unreachable with all-allowed names (an
        // unknown 9th name trips field_unknown first). We assert that defence
        // in depth here: 9 distinct names yields a typed rejection.
        let mut map = String::from(r#"{"state":"degraded","reason":"slow","items":[{"#);
        for i in 0..(SNAPSHOT_MAX_ITEM_FIELDS + 1) {
            if i > 0 { map.push(','); }
            map.push_str(&format!(r#""label{i}":"v""#));
        }
        map.push_str(r#"}]}"#);
        let err = serde_json::from_str::<SectionV1>(&map).unwrap_err();
        assert!(err.to_string().contains("snapshot_item_field_unknown"), "{}", err);

        // Bare primitive items intentionally parse here: `hub_runtime`'s fetch
        // path checks `!item.is_object()` and returns the certified
        // `snapshot_bounds_invalid`. Deserialization MUST NOT reject them.
        let primitives = r#"{"state":"degraded","reason":"slow","items":["content"]}"#;
        let section: SectionV1 = serde_json::from_str(primitives).unwrap();
        assert_eq!(section.items.as_ref().unwrap().first().unwrap().as_str(), Some("content"));
    }
    #[test]
    fn production_fetch_parse_rejects_nested_and_oversized_item_values() {
        // `hub_runtime::fetch_snapshot` deserializes the fetched body into a
        // SnapshotV2 whose sections are `BTreeMap<String, SectionV1>` (this
        // crate's SectionV1, with `deserialize_bounded_items`). This test
        // mirrors that parse against `schema_types::SnapshotV2` to prove the
        // production fetch path rejects content-bearing items, not just the
        // standalone section parser.
        let valid = r#"{"schemaVersion":2,"productId":"cortex","observedAtUnixMs":1,"sections":{"x":{"state":"available","reason":"ok","items":[{"label":"d1","count":3}]}}}"#;
        serde_json::from_str::<SnapshotV1>(valid).unwrap();

        for (bad, needle) in [
            (r#"{"schemaVersion":2,"productId":"cortex","observedAtUnixMs":1,"sections":{"x":{"state":"available","reason":"ok","items":[{"label":"d1","count":{"a":1}}]}}}"#, "snapshot_item_value_not_primitive"),
            (r#"{"schemaVersion":2,"productId":"cortex","observedAtUnixMs":1,"sections":{"x":{"state":"available","reason":"ok","items":[{"label":"d1","rogue":"v"}]}}}"#, "snapshot_item_field_unknown"),
            (format!(r#"{{"schemaVersion":2,"productId":"cortex","observedAtUnixMs":1,"sections":{{"x":{{"state":"available","reason":"ok","items":[{{"label":"{}"}}]}}}}}}"#, "x".repeat(SNAPSHOT_MAX_ITEM_LABEL_BYTES + 1)).as_str(), "snapshot_item_string_too_long"),
        ] {
            let err = serde_json::from_str::<SnapshotV1>(bad).unwrap_err();
            assert!(err.to_string().contains(needle), "expected {needle}, got {err}");
        }

        // A bare-primitive item still parses here: `hub_runtime`'s fetch then
        // rejects it with the certified `snapshot_bounds_invalid` (its own
        // `!item.is_object()` check), which this deserializer must not preempt.
        let primitives = r#"{"schemaVersion":2,"productId":"cortex","observedAtUnixMs":1,"sections":{"x":{"state":"available","reason":"ok","items":["content"]}}}"#;
        serde_json::from_str::<SnapshotV1>(primitives).unwrap();
    }
    #[test]
    fn production_fetch_parse_rejects_wrong_item_scalar_types() {
        // The deserializer must reject every invalid item shape the released
        // snapshot.v2 schema and the Node validator reject: missing/null label,
        // wrong scalar types, negative count, invalid severity, fractional /
        // negative observedAtUnixMs, and non-boolean stale. None of these reach
        // `hub_runtime`'s fetch path: they fail at parse time as
        // `snapshot_schema_invalid`. Error code strings mirror
        // `schema/validate.mjs` so the two validators agree.
        let prefix = r#"{"schemaVersion":2,"productId":"cortex","observedAtUnixMs":1,"sections":{"x":{"state":"available","reason":"ok","items":["#;
        let suffix = r#"]}}}}"#;
        // (bad-item, expected_code) — codes mirror schema/validate.mjs.
        for (bad, needle) in [
            // missing/null label
            (r#"{}"#, "snapshot_item_string_too_long"),
            (r#"{"label":null}"#, "snapshot_item_string_too_long"),
            (r#"{"label":""}"#, "snapshot_item_string_too_long"),
            // wrong scalar types
            (r#"{"label":"d1","count":"3"}"#, "snapshot_item_value_not_primitive"),
            (r#"{"label":"d1","count":true}"#, "snapshot_item_value_not_primitive"),
            (r#"{"label":"d1","count":null}"#, "snapshot_item_value_not_primitive"),
            (r#"{"label":"d1","severity":123}"#, "snapshot_item_value_not_primitive"),
            (r#"{"label":"d1","stale":"yes"}"#, "snapshot_item_value_not_primitive"),
            (r#"{"label":"d1","kind":7}"#, "snapshot_item_string_too_long"),
            (r#"{"label":"d1","evidence":false}"#, "snapshot_item_string_too_long"),
            // invalid severity enum
            (r#"{"label":"d1","severity":"bogus"}"#, "snapshot_item_value_not_primitive"),
            // negative count (schema minimum 0)
            (r#"{"label":"d1","count":-1}"#, "snapshot_item_value_not_primitive"),
            // fractional / negative observedAtUnixMs (schema: integer, minimum 0)
            (r#"{"label":"d1","observedAtUnixMs":1.5}"#, "snapshot_item_value_not_primitive"),
            (r#"{"label":"d1","observedAtUnixMs":-1}"#, "snapshot_item_value_not_primitive"),
            (r#"{"label":"d1","observedAtUnixMs":"now"}"#, "snapshot_item_value_not_primitive"),
            // non-boolean stale
            (r#"{"label":"d1","stale":0}"#, "snapshot_item_value_not_primitive"),
        ] {
            let json = format!("{prefix}{bad}{suffix}");
            let err = serde_json::from_str::<SnapshotV1>(&json).unwrap_err();
            assert!(
                err.to_string().contains(needle),
                "expected {needle}, got {err} for item {bad}"
            );
        }
        // A fully valid closed item still round-trips.
        let valid = r#"{"schemaVersion":2,"productId":"cortex","observedAtUnixMs":1,"sections":{"x":{"state":"degraded","reason":"slow","items":[{"label":"d1","kind":"delivery","count":3,"severity":"warning","evidence":"hub#1","resolver":"retry","observedAtUnixMs":999,"stale":false}]}}}"#;
        let snap: SnapshotV1 = serde_json::from_str(valid).unwrap();
        let res = serde_json::to_string(&snap).unwrap();
        let de: SnapshotV1 = serde_json::from_str(&res).unwrap();
        assert_eq!(snap, de);

        // Bare-primitive (non-object) items still parse here: `hub_runtime`'s
        // own `!item.is_object()` check returns the certified `snapshot_bounds_invalid`.
        let primitives = r#"{"schemaVersion":2,"productId":"cortex","observedAtUnixMs":1,"sections":{"x":{"state":"available","reason":"ok","items":["content"]}}}"#;
        serde_json::from_str::<SnapshotV1>(primitives).unwrap();
    }
    #[test]
    fn snapshot_with_optional_stale_fields() {
        let json = r#"{"schemaVersion":2,"productId":"cortex","observedAtUnixMs":42,"sections":{"memory":{"state":"unavailable","reason":"offline"}},"stale":true,"cacheAgeMs":1000}"#;
        let snap: SnapshotV1 = serde_json::from_str(json).unwrap();
        assert_eq!(snap.stale, Some(true));
        assert_eq!(snap.cache_age_ms, Some(1000));
        let ser = serde_json::to_string(&snap).unwrap();
        assert!(ser.contains("stale"));
        assert!(ser.contains("cacheAgeMs"));
    }
}