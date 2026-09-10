//! Parity test for `blueprint/src/lib/hooks/blueprint-graph-index.mjs` ->
//! `membrane_blueprint::lib_hooks_graph_index`.

use membrane_blueprint::lib_hooks_graph_index::{dispatch_graph_index_event, GraphIndexOutcome};

#[test]
fn indexes_only_on_post_tool_use_stop_and_session_end() {
    for kind in ["PostToolUse", "Stop", "SessionEnd"] {
        let outcome = dispatch_graph_index_event(kind, || Some("indexed".to_string()));
        assert_eq!(
            outcome,
            GraphIndexOutcome::Indexed {
                result: Some("indexed".to_string())
            }
        );
    }
}

#[test]
fn every_other_event_is_skipped_without_invoking_indexer() {
    for kind in ["PreToolUse", "UserPromptSubmit", "Notification"] {
        let outcome = dispatch_graph_index_event(kind, || {
            panic!("indexer must not run for ignored event {kind}")
        });
        assert_eq!(
            outcome,
            GraphIndexOutcome::Skipped {
                reason: "event_ignored"
            }
        );
    }
}
