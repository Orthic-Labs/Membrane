use serde_json::{json, Value};
use std::collections::HashSet;

pub(crate) fn definitions() -> Value {
    json!([])
}

pub(crate) fn negotiated_definitions(params: Option<&Value>) -> Value {
    let requested = params
        .and_then(|value| value.pointer("/_meta/membrane.toolsets.v1"))
        .and_then(Value::as_array);
    let valid = requested
        .map(|groups| {
            let mut seen = HashSet::new();
            groups.iter().all(|group| {
                matches!(
                    group.as_str(),
                    Some("default" | "memory" | "blueprint" | "diagnostic")
                ) && seen.insert(group)
            })
        })
        .unwrap_or(true);
    if valid {
        definitions()
    } else {
        definitions()
    }
}
