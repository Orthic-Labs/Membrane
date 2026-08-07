use crate::digest_str;
use serde::{de::Error as _, Deserialize, Deserializer, Serialize};
pub const COMPRESSION_RECEIPT_SCHEMA_VERSION: u32 = 1;
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct NonEmptyString(String);

impl NonEmptyString {
    pub fn new(value: impl Into<String>) -> Result<Self, &'static str> {
        let value = value.into();
        if value.is_empty() {
            Err("value must not be empty")
        } else {
            Ok(Self(value))
        }
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
impl<'de> Deserialize<'de> for NonEmptyString {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Self::new(String::deserialize(deserializer)?).map_err(D::Error::custom)
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SpanV1 {
    pub start: u32,
    pub end: u32,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DroppedSpanV1 {
    pub span: SpanV1,
    pub reason: NonEmptyString,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CompressionReceiptV1 {
    #[serde(deserialize_with = "deserialize_schema_version")]
    pub schema_version: u32,
    #[serde(deserialize_with = "deserialize_sha256")]
    pub original_hash: String,
    pub transform: NonEmptyString,
    pub transform_version: NonEmptyString,
    pub kept_spans: Vec<SpanV1>,
    pub protected_spans: Vec<SpanV1>,
    pub dropped: Vec<DroppedSpanV1>,
    pub resolver: NonEmptyString,
    pub risk: NonEmptyString,
    pub token_delta: i64,
}

fn deserialize_schema_version<'de, D: Deserializer<'de>>(deserializer: D) -> Result<u32, D::Error> {
    let value = u32::deserialize(deserializer)?;
    if value == COMPRESSION_RECEIPT_SCHEMA_VERSION {
        Ok(value)
    } else {
        Err(D::Error::custom("schemaVersion must equal 1"))
    }
}

fn deserialize_sha256<'de, D: Deserializer<'de>>(deserializer: D) -> Result<String, D::Error> {
    let value = String::deserialize(deserializer)?;
    let valid = value.len() == 71
        && value.starts_with("sha256:")
        && value[7..]
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte));
    if valid {
        Ok(value)
    } else {
        Err(D::Error::custom(
            "originalHash must be sha256:<64 lowercase hex>",
        ))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum ImmutableSourceReason {
    #[error("resolver_unavailable")]
    ResolverUnavailable,
    #[error("original_hash_mismatch")]
    OriginalHashMismatch,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("immutable source unavailable for {resolver} ({original_hash}): {reason}")]
pub struct ImmutableSourceError {
    pub resolver: String,
    pub original_hash: String,
    pub reason: ImmutableSourceReason,
}

impl CompressionReceiptV1 {
    pub const SCHEMA_VERSION: u32 = COMPRESSION_RECEIPT_SCHEMA_VERSION;

    pub fn is_lossy(&self) -> bool {
        !self.dropped.is_empty()
    }

    pub fn recover_if_lossy<F>(&self, resolve: F) -> Result<Option<String>, ImmutableSourceError>
    where
        F: FnOnce(&str) -> Option<String>,
    {
        if !self.is_lossy() {
            return Ok(None);
        }
        let original = resolve(self.resolver.as_str()).ok_or_else(|| ImmutableSourceError {
            resolver: self.resolver.as_str().to_string(),
            original_hash: self.original_hash.clone(),
            reason: ImmutableSourceReason::ResolverUnavailable,
        })?;
        if digest_str(&original) != self.original_hash {
            return Err(ImmutableSourceError {
                resolver: self.resolver.as_str().to_string(),
                original_hash: self.original_hash.clone(),
                reason: ImmutableSourceReason::OriginalHashMismatch,
            });
        }
        Ok(Some(original))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn receipt(original: &str) -> CompressionReceiptV1 {
        CompressionReceiptV1 {
            schema_version: 1,
            original_hash: digest_str(original),
            transform: NonEmptyString::new("head-tail").unwrap(),
            transform_version: NonEmptyString::new("1").unwrap(),
            kept_spans: vec![SpanV1 { start: 0, end: 2 }],
            protected_spans: vec![],
            dropped: vec![DroppedSpanV1 {
                span: SpanV1 { start: 2, end: 4 },
                reason: NonEmptyString::new("budget").unwrap(),
            }],
            resolver: NonEmptyString::new("source-read").unwrap(),
            risk: NonEmptyString::new("medium").unwrap(),
            token_delta: -2,
        }
    }

    #[test]
    fn lossy_receipt_recovers_hash_bound_original() {
        let value = receipt("original");
        assert_eq!(
            value.recover_if_lossy(|_| Some("original".into())).unwrap(),
            Some("original".into())
        );
    }

    #[test]
    fn unavailable_or_tampered_original_is_typed_error() {
        let value = receipt("original");
        assert_eq!(
            value.recover_if_lossy(|_| None).unwrap_err().reason,
            ImmutableSourceReason::ResolverUnavailable
        );
        assert_eq!(
            value
                .recover_if_lossy(|_| Some("tampered".into()))
                .unwrap_err()
                .reason,
            ImmutableSourceReason::OriginalHashMismatch
        );
    }

    #[test]
    fn canonical_fixture_round_trips_and_rejects_invalid_contract_values() {
        let json = r#"{"dropped":[{"reason":"budget","span":{"end":4,"start":2}}],"keptSpans":[{"end":2,"start":0}],"originalHash":"sha256:0682c5f2076f099c34cfdd15a9e063849ed437a49677e6fcc5b4198c76575be5","protectedSpans":[],"resolver":"source-read","risk":"medium","schemaVersion":1,"tokenDelta":-2,"transform":"head-tail","transformVersion":"1"}"#;
        let receipt: CompressionReceiptV1 = serde_json::from_str(json).unwrap();
        assert_eq!(crate::canonical_json_of(&receipt), json);
        assert!(serde_json::from_str::<CompressionReceiptV1>(
            &json.replace("\"schemaVersion\":1", "\"schemaVersion\":2")
        )
        .is_err());
        assert!(
            serde_json::from_str::<CompressionReceiptV1>(&json.replace("0682c5", "ABCDEF"))
                .is_err()
        );
        assert!(
            serde_json::from_str::<CompressionReceiptV1>(&json.replace("head-tail", "")).is_err()
        );
        assert!(serde_json::from_str::<CompressionReceiptV1>(&json.replace("budget", "")).is_err());
    }

    #[test]
    fn nonempty_values_cannot_be_constructed_or_serialized_empty() {
        assert!(NonEmptyString::new("").is_err());
        let value = NonEmptyString::new("valid").unwrap();
        assert_eq!(serde_json::to_string(&value).unwrap(), "\"valid\"");
        assert!(serde_json::from_str::<NonEmptyString>("\"\"").is_err());
    }
}
