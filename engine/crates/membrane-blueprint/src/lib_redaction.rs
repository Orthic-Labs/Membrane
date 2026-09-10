//! Native Rust port of `blueprint/src/lib/redaction.mjs`.
//!
//! Lane LIB4 (r5 closure): this module has no prior native equivalent.
//! Ported behavior: `redact_for_egress` walks a `serde_json::Value` tree,
//! redacting object values whose key names look like secrets, and scrubbing
//! secret-shaped string values (tokens, private keys, URL passwords, raw
//! 40-char base64) unless the key is content-addressed (a commit SHA,
//! digest, fingerprint, etc.), matching the legacy JS heuristics exactly.

use regex::Regex;
use serde_json::Value;
use std::sync::OnceLock;

fn secret_key() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(
            r"(?i)token|secret|password|passwd|api[_-]?key|authorization|cookie|private[_-]?key|client_email",
        )
        .unwrap()
    })
}

fn secret_value() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(
            r"(?:gh[pousr]_[A-Za-z0-9_]{20,}|github_pat_[A-Za-z0-9_]{30,}|npm_[A-Za-z0-9]{30,}|AKIA[0-9A-Z]{16}|-----BEGIN [A-Z ]*PRIVATE KEY-----|Bearer\s+[A-Za-z0-9._~-]+|sk-ant-[A-Za-z0-9_-]{20,}|sk-[A-Za-z0-9_-]{20,}|xox[abpr]-[A-Za-z0-9-]{10,}|eyJ[A-Za-z0-9_-]{10,}\.[A-Za-z0-9_-]{10,}\.[A-Za-z0-9._~/+=-]{10,})",
        )
        .unwrap()
    })
}

fn secret_url_password() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(
            r"(?i)(?:postgres|postgresql|mysql|mongodb(?:\+srv)?|redis)://[^:\s]+:([^@\s]+)@",
        )
        .unwrap()
    })
}

fn secret_raw_base64_40() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"\b[A-Za-z0-9/+=]{40}\b").unwrap())
}

fn content_addressed_key() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(
            r"(?i)revision|fingerprint|digest|generation|content_?hash|^sha$|_sha$|checksum|integrity",
        )
        .unwrap()
    })
}

/// Redact secret-shaped fields and values from a JSON value tree, mirroring
/// `redactForEgress` in the legacy JS module.
///
/// `content_addressed` mirrors the JS default-parameter `contentAddressed`
/// (defaults to `false` at the top level; propagated per-field by key name
/// when recursing into an object).
pub fn redact_for_egress(value: &Value, content_addressed: bool) -> Value {
    match value {
        Value::Array(items) => Value::Array(
            items
                .iter()
                .map(|item| redact_for_egress(item, content_addressed))
                .collect(),
        ),
        Value::Object(map) => {
            let mut out = serde_json::Map::with_capacity(map.len());
            for (key, item) in map.iter() {
                let redacted = if secret_key().is_match(key) {
                    Value::String("[REDACTED]".to_string())
                } else {
                    redact_for_egress(item, content_addressed_key().is_match(key))
                };
                out.insert(key.clone(), redacted);
            }
            Value::Object(out)
        }
        Value::String(s) => Value::String(redact_string(s, content_addressed)),
        other => other.clone(),
    }
}

fn redact_string(value: &str, content_addressed: bool) -> String {
    let mut out = secret_value().replace_all(value, "[REDACTED]").into_owned();
    out = secret_url_password()
        .replace_all(&out, |caps: &regex::Captures| {
            let whole = caps.get(0).unwrap().as_str();
            let pw = caps.get(1).unwrap().as_str();
            whole.replacen(pw, "[REDACTED]", 1)
        })
        .into_owned();
    if !content_addressed {
        out = secret_raw_base64_40()
            .replace_all(&out, "[REDACTED]")
            .into_owned();
    }
    out
}
