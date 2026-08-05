"""Membrane federation gateway — Python orchestration layer.

This module is the SOLE owner of provider fan-out per dispatch §G3A + §G5.
Clients (Claude, Codex, MCP) submit only (task, repo_root, client, session,
max_tokens, anchors, scope_grant_id); the gateway invokes each provider
adapter in parallel, merges their ContextCandidateSet v1 entries into one
canonical CCS, and writes that CCS to stdout. The Rust dispatcher
(`crypt federate`) then runs the existing deterministic admission and
emits the final planner envelope.

Provider payload formats and SQLite details never enter client adapters.
Crypt durable storage is never modified by this module.
"""

__all__ = [
    "FederationRequest",
    "FederationResult",
    "federate",
    "main",
]
