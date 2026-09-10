//! Native Rust port of `blueprint/src/lib/hooks/blueprint-graph-index.mjs`.
//!
//! Lane LIB3 (r5 closure): no prior native equivalent found for this exact
//! hook-dispatch contract (grep of membrane-blueprint/src and
//! membrane-runtime/src turned up no `blueprint_graph_index` kind or an
//! `INDEX_EVENTS`-shaped event filter). Ported behavior: the hook only acts
//! on `PostToolUse`, `Stop`, and `SessionEnd` events; every other event is
//! reported as `skipped`/`event_ignored` without invoking the indexer.

use std::collections::HashSet;
use std::sync::OnceLock;

fn index_events() -> &'static HashSet<&'static str> {
    static SET: OnceLock<HashSet<&'static str>> = OnceLock::new();
    SET.get_or_init(|| {
        ["PostToolUse", "Stop", "SessionEnd"]
            .into_iter()
            .collect()
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GraphIndexOutcome {
    Skipped { reason: &'static str },
    Indexed { result: Option<String> },
}

/// Mirrors `createBlueprintGraphIndexHook(...).handle(event, context)`: only
/// `PostToolUse`/`Stop`/`SessionEnd` events reach the indexer; every other
/// event kind is skipped with `event_ignored`.
pub fn dispatch_graph_index_event<F>(event_kind: &str, index: F) -> GraphIndexOutcome
where
    F: FnOnce() -> Option<String>,
{
    if !index_events().contains(event_kind) {
        return GraphIndexOutcome::Skipped {
            reason: "event_ignored",
        };
    }
    GraphIndexOutcome::Indexed { result: index() }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn indexes_on_recognized_events() {
        for kind in ["PostToolUse", "Stop", "SessionEnd"] {
            let outcome = dispatch_graph_index_event(kind, || Some("ok".to_string()));
            assert_eq!(
                outcome,
                GraphIndexOutcome::Indexed {
                    result: Some("ok".to_string())
                }
            );
        }
    }

    #[test]
    fn skips_unrecognized_events() {
        let outcome = dispatch_graph_index_event("PreToolUse", || {
            panic!("indexer must not run for ignored events")
        });
        assert_eq!(
            outcome,
            GraphIndexOutcome::Skipped {
                reason: "event_ignored"
            }
        );
    }
}
