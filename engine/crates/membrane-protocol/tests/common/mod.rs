//! A minimal JSON-Schema validator covering exactly the subset of draft 2020-12
//! that the Membrane v1 contract schemas use: `type`, `required`, `properties`,
//! `additionalProperties`, `enum`, `const`, `pattern`, `minLength`, `minimum`,
//! `maximum`, `items`, `format` (date-time), and local `$ref`/`$defs`.
//!
//! This is intentionally NOT a general validator — it enforces the contract
//! shapes and nothing more, so the round-trip tests stay dependency-free. The
//! TypeScript binding implements the same subset and must agree.

use serde_json::Value;
use std::collections::HashSet;

/// Validate `instance` against `schema`, returning human-readable violations
/// (empty when valid).
pub fn validate(schema: &Value, instance: &Value) -> Vec<String> {
    let mut errors = Vec::new();
    validate_into(schema, instance, schema, "$", &mut errors);
    errors
}

fn resolve<'a>(root: &'a Value, schema: &'a Value) -> &'a Value {
    if let Some(reference) = schema.get("$ref").and_then(Value::as_str) {
        if let Some(path) = reference.strip_prefix("#/") {
            let mut node = root;
            for segment in path.split('/') {
                node = &node[segment];
            }
            return node;
        }
    }
    schema
}

fn validate_into(
    schema: &Value,
    instance: &Value,
    root: &Value,
    path: &str,
    errors: &mut Vec<String>,
) {
    let schema = resolve(root, schema);

    if let Some(expected) = schema.get("const") {
        if instance != expected {
            errors.push(format!("{path}: const mismatch, expected {expected}"));
        }
    }
    if let Some(variants) = schema.get("enum").and_then(Value::as_array) {
        if !variants.contains(instance) {
            errors.push(format!("{path}: not in enum {variants:?}"));
        }
    }
    if let Some(type_value) = schema.get("type") {
        let types: Vec<&str> = match type_value {
            Value::String(s) => vec![s.as_str()],
            Value::Array(arr) => arr.iter().filter_map(Value::as_str).collect(),
            _ => vec![],
        };
        if !types.is_empty() && !types.iter().any(|t| type_matches(t, instance)) {
            errors.push(format!("{path}: type mismatch, expected one of {types:?}"));
            return; // further checks assume the type
        }
    }

    match instance {
        Value::Object(map) => validate_object(schema, map, root, path, errors),
        Value::Array(items) => {
            if let Some(item_schema) = schema.get("items") {
                for (index, item) in items.iter().enumerate() {
                    let child = format!("{path}[{index}]");
                    validate_into(item_schema, item, root, &child, errors);
                }
            }
        }
        Value::String(text) => validate_string(schema, text, path, errors),
        Value::Number(number) => validate_number(schema, number, path, errors),
        _ => {}
    }
}

fn validate_object(
    schema: &Value,
    map: &serde_json::Map<String, Value>,
    root: &Value,
    path: &str,
    errors: &mut Vec<String>,
) {
    if let Some(required) = schema.get("required").and_then(Value::as_array) {
        for key in required.iter().filter_map(Value::as_str) {
            if !map.contains_key(key) {
                errors.push(format!("{path}: missing required key `{key}`"));
            }
        }
    }
    let properties = schema.get("properties").and_then(Value::as_object);
    let mut allowed: HashSet<&str> = HashSet::new();
    if let Some(properties) = properties {
        for (key, subschema) in properties {
            allowed.insert(key.as_str());
            if let Some(value) = map.get(key) {
                let child = format!("{path}.{key}");
                validate_into(subschema, value, root, &child, errors);
            }
        }
    }
    match schema.get("additionalProperties") {
        Some(Value::Bool(false)) => {
            for key in map.keys() {
                if !allowed.contains(key.as_str()) {
                    errors.push(format!("{path}: unexpected additional key `{key}`"));
                }
            }
        }
        Some(subschema @ Value::Object(_)) => {
            for (key, value) in map {
                if !allowed.contains(key.as_str()) {
                    let child = format!("{path}.{key}");
                    validate_into(subschema, value, root, &child, errors);
                }
            }
        }
        _ => {}
    }
}

fn validate_string(schema: &Value, text: &str, path: &str, errors: &mut Vec<String>) {
    if let Some(min) = schema.get("minLength").and_then(Value::as_u64) {
        if (text.chars().count() as u64) < min {
            errors.push(format!("{path}: shorter than minLength {min}"));
        }
    }
    if let Some(pattern) = schema.get("pattern").and_then(Value::as_str) {
        if !pattern_matches(pattern, text) {
            errors.push(format!("{path}: does not match pattern {pattern}"));
        }
    }
    if schema.get("format").and_then(Value::as_str) == Some("date-time") && !is_date_time(text) {
        errors.push(format!("{path}: not an RFC 3339 date-time"));
    }
}

fn validate_number(
    schema: &Value,
    number: &serde_json::Number,
    path: &str,
    errors: &mut Vec<String>,
) {
    let value = number.as_f64().unwrap_or(f64::NAN);
    if let Some(min) = schema.get("minimum").and_then(Value::as_f64) {
        if value < min {
            errors.push(format!("{path}: below minimum {min}"));
        }
    }
    if let Some(max) = schema.get("maximum").and_then(Value::as_f64) {
        if value > max {
            errors.push(format!("{path}: above maximum {max}"));
        }
    }
}

fn type_matches(expected: &str, instance: &Value) -> bool {
    match expected {
        "object" => instance.is_object(),
        "array" => instance.is_array(),
        "string" => instance.is_string(),
        "boolean" => instance.is_boolean(),
        "null" => instance.is_null(),
        "integer" => instance.as_i64().is_some() || instance.as_u64().is_some(),
        "number" => instance.is_number(),
        _ => false,
    }
}

/// Dependency-free matcher for the handful of anchored patterns the contract
/// schemas use. Each recognized pattern is a literal prefix plus a trailing
/// character-class repeat; anything unrecognized falls back to a conservative
/// substring check.
fn pattern_matches(pattern: &str, text: &str) -> bool {
    if pattern == "^sha256:[a-f0-9]{64}$" {
        return match text.strip_prefix("sha256:") {
            Some(rest) => rest.len() == 64 && rest.bytes().all(is_lower_hex),
            None => false,
        };
    }
    if pattern == "^scope-grant-ed25519-v1:[a-f0-9]{32}$" {
        return match text.strip_prefix("scope-grant-ed25519-v1:") {
            Some(rest) => rest.len() == 32 && rest.bytes().all(is_lower_hex),
            None => false,
        };
    }
    if pattern == "^sgv1-" {
        return text.starts_with("sgv1-");
    }
    text.contains(pattern.trim_start_matches('^').trim_end_matches('$'))
}

fn is_lower_hex(byte: u8) -> bool {
    byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)
}

/// Minimal RFC 3339 / ISO 8601 date-time check (`YYYY-MM-DDTHH:MM:SS[.fff]Z`).
fn is_date_time(text: &str) -> bool {
    let bytes = text.as_bytes();
    if bytes.len() < 20 {
        return false;
    }
    let digit = |i: usize| bytes.get(i).is_some_and(u8::is_ascii_digit);
    let core = digit(0)
        && digit(1)
        && digit(2)
        && digit(3)
        && bytes[4] == b'-'
        && digit(5)
        && digit(6)
        && bytes[7] == b'-'
        && digit(8)
        && digit(9)
        && bytes[10] == b'T'
        && digit(11)
        && digit(12)
        && bytes[13] == b':'
        && digit(14)
        && digit(15)
        && bytes[16] == b':'
        && digit(17)
        && digit(18);
    core && text.ends_with('Z')
}
