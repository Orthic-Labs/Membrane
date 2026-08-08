"""Deterministic proof that overlay identity stays bound to each CCS."""

from engine.federation import gateway


def _freshness(session: str, worktree: str, digest: str) -> dict:
    return {
        "indexedAt": "2026-08-08T00:00:00Z",
        "stale": False,
        "graphState": "dirty_overlay",
        "generationId": "gen-shared",
        "overlayDigest": digest,
        "overlayIdentity": {
            "session_id": session,
            "worktree_path": worktree,
            "generation_id": "gen-shared",
            "overlay_digest": digest,
        },
    }


def _assemble(session: str, worktree: str, digest: str) -> dict:
    candidate = {
        "id": f"graph:{session}",
        "sourceKind": "graph",
        "sourceRef": "src/lib.rs",
        "sourceHash": "sha256:" + "a" * 64,
        "resolver": "cortex graph resolve",
    }
    return gateway._merge_candidates(
        [("cortex", [candidate], [])],
        _freshness(session, worktree, digest),
        gateway.Path(worktree),
        "task",
        "trace",
        128,
    )


def test_concurrent_worktrees_cannot_reuse_overlay_identity():
    first = _assemble("session-a", "/workspace/a", "sha256:" + "1" * 64)
    second = _assemble("session-b", "/workspace/b", "sha256:" + "2" * 64)
    assert first["freshness"]["overlayIdentity"]["session_id"] == "session-a"
    assert second["freshness"]["overlayIdentity"]["session_id"] == "session-b"
    assert first["freshness"]["overlayIdentity"]["worktree_path"] != second["freshness"]["overlayIdentity"]["worktree_path"]
    assert first["freshness"]["overlayIdentity"]["overlay_digest"] != second["freshness"]["overlayIdentity"]["overlay_digest"]
