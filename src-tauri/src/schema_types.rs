use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Mirrors `schema/manifest.v1.ts` and `schema/manifest.v1.schema.json`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ManifestV1 {
    pub schema_version: u32,
    pub product_id: String,
    pub display_name: String,
    pub product_version: String,
    pub hub_compat_range: String,
    pub install_root: String,
    pub service_start: Vec<String>,
    pub service_stop: Vec<String>,
    pub status_endpoint: StatusEndpoint,
    pub icon: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StatusEndpoint {
    pub host: String,
    pub port: u16,
    pub auth_header: String,
    pub auth_token: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Copy)]
#[serde(rename_all = "lowercase")]
pub enum SectionState {
    Available,
    Degraded,
    Unavailable,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SectionV1 {
    pub state: SectionState,
    pub reason: String,
    pub items: Option<Vec<serde_json::Value>>,
    pub evidence: Option<String>,
    pub resolver: Option<String>,
    pub observed_at_unix_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SnapshotV1 {
    pub schema_version: u32,
    pub product_id: String,
    pub observed_at_unix_ms: u64,
    pub sections: HashMap<String, SectionV1>,
    pub stale: Option<bool>,
    pub cache_age_ms: Option<u64>,
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn snapshot_round_trip() {
        let json = r#"{"schemaVersion":1,"productId":"membrane","observedAtUnixMs":123,"sections":{"deliveries":{"state":"available","reason":"ok"}}}"#;
        let snap: SnapshotV1 = serde_json::from_str(json).unwrap();
        assert_eq!(snap.schema_version, 1);
        assert_eq!(snap.sections["deliveries"].state, SectionState::Available);
        let ser = serde_json::to_string(&snap).unwrap();
        let de: SnapshotV1 = serde_json::from_str(&ser).unwrap();
        assert_eq!(snap, de);
    }
    #[test]
    fn manifest_round_trip() {
        let json = r#"{"schemaVersion":1,"productId":"cortex","displayName":"Cortex","productVersion":"1.0.0","hubCompatRange":">=0.1.0","installRoot":"/tmp/cortex","serviceStart":["/tmp/cortex/bin"],"serviceStop":[],"statusEndpoint":{"host":"127.0.0.1","port":8080,"authHeader":"X-Token","authToken":"secret"},"icon":"/tmp/cortex/icon.png"}"#;
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
    fn section_optional_fields_round_trip() {
        let json = r#"{"state":"degraded","reason":"slow","items":[1,"a",true],"evidence":"log line","resolver":"retry","observedAtUnixMs":999}"#;
        let section: SectionV1 = serde_json::from_str(json).unwrap();
        assert_eq!(section.items.as_ref().unwrap().len(), 3);
        assert_eq!(section.evidence.as_deref(), Some("log line"));
        let ser = serde_json::to_string(&section).unwrap();
        let de: SectionV1 = serde_json::from_str(&ser).unwrap();
        assert_eq!(section, de);
    }
    #[test]
    fn snapshot_with_optional_stale_fields() {
        let json = r#"{"schemaVersion":1,"productId":"cortex","observedAtUnixMs":42,"sections":{"memory":{"state":"unavailable","reason":"offline"}},"stale":true,"cacheAgeMs":1000}"#;
        let snap: SnapshotV1 = serde_json::from_str(json).unwrap();
        assert_eq!(snap.stale, Some(true));
        assert_eq!(snap.cache_age_ms, Some(1000));
        let ser = serde_json::to_string(&snap).unwrap();
        assert!(ser.contains("stale"));
        assert!(ser.contains("cacheAgeMs"));
    }
}
