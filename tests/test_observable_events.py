from adapt.observable_events import consume_observable_events
import unittest


def event(event_id, event_type, origin="host", complete=True):
    return {
        "schema": "orthic.observable-event.v1", "installation_id": "i", "client_id": "claude_code",
        "session_id": "s", "task_id": "t", "turn_id": "u", "trace_id": "x", "event_id": event_id,
        "event_type": event_type, "origin": origin, "content_ref_or_digest": "sha256:" + "a" * 64,
        "timestamp": "2026-08-01T00:00:00Z", "completeness": {"packet": complete, "receipt": complete},
        "policy_snapshot_digest": "sha256:" + "b" * 64,
    }


class ObservableEventTests(unittest.TestCase):
    def test_consumer_preserves_lineage_and_remains_metadata_only(self):
        result = consume_observable_events([event("1", "packet_delivered"), event("2", "user_correction", "user"), event("3", "tool_receipt")])
        self.assertEqual([row["event_id"] for row in result["lineage"]["i|s|t|u|x"]], ["1", "2", "3"])
        self.assertEqual(result["taste_candidates"], [])
        self.assertFalse(result["insights"])


    def test_consumer_emits_four_deterministic_failure_patterns(self):
        missing = event("1", "context_requested")
        missing["session_id"] = "missing-session"
        events = [missing, event("2", "packet_delivered", complete=False), event("3", "tool_receipt", complete=False)]
        events.extend(event(str(index), "tool_receipt_failed") for index in range(4, 7))
        self.assertEqual(consume_observable_events(events)["insights"], [
            "missing_context_delivery", "degraded_context_delivery", "incomplete_tool_receipt", "repeated_tool_failure",
        ])
