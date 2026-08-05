"""morph — Orthic Morph Taste entry (backward-compatible facade).

Implementation is split along product boundaries:
  - ``taste``       durable preferences → Crypt (apply_actions, add_rule, …)
  - ``taste_apply`` reviewed manifest apply (zero LLM)
  - ``taste_mine``  extract/synth orchestration helpers
  - ``cli``         flag-based Taste CLI + ``doctor`` dispatch
  - ``doctor``      multiwriter conformance (Cortex/Sentinel = not-yet)
  - ``workspace_runtime`` parent Crypt/session/mirror import boundary

CLI entry points:
  python3 morph.py …          # legacy Taste flags
  python3 morph.py doctor …   # Morph Doctor
  python3 morph.py …          # Orthic Morph display alias (same dispatch)
"""
from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import morph_llm  # noqa: E402
import morph_sessions as ts  # noqa: E402
import outcomes  # noqa: E402
try:
    import run_journal  # noqa: E402
except ImportError:
    run_journal = None
try:
    import admission  # noqa: E402
except ImportError:
    admission = None
import preference_record  # noqa: E402
import manifest  # noqa: E402
import authority  # noqa: E402
import workspace_runtime  # noqa: E402

# Ensure parent tools/lib is importable for core_compiler / memory.*
_TOOLS_LIB = workspace_runtime.workspace_root() / "tools" / "lib"
if str(_TOOLS_LIB) not in sys.path:
    sys.path.insert(0, str(_TOOLS_LIB))

import core_compiler  # noqa: E402
import rollback  # noqa: E402
import cross_machine  # noqa: E402
import morph_persistence  # noqa: E402
import rule_key  # noqa: E402

import taste  # noqa: E402
import taste_apply  # noqa: E402
import taste_mine  # noqa: E402
import cli  # noqa: E402
# doctor is optional at import time (pulls conformance); expose lazily via attribute
import doctor  # noqa: E402
import shutil  # noqa: E402
import subprocess  # noqa: E402

# --- re-exports (tests + callers keep `import morph`) ---
RULES_FILE = taste.RULES_FILE
DIGEST_FILE = taste.DIGEST_FILE
WORKSPACE_ROOT = taste.WORKSPACE_ROOT
CRYPT_MUTATION_TIMEOUT_SECONDS = taste.CRYPT_MUTATION_TIMEOUT_SECONDS

_installation_file = taste._installation_file
_multiwriter_context = taste._multiwriter_context
_qualified_session_sources = taste._qualified_session_sources
_rules_path = taste._rules_path
_digest_path = taste._digest_path
_audit_file = taste._audit_file
_run_crypt = taste._run_crypt
preflight_apply = taste.preflight_apply
initialized = taste.initialized
rule_body = taste.rule_body
_safe_retrieval_aliases = taste._safe_retrieval_aliases
load_rules = taste.load_rules
save_rules = taste.save_rules
write_digest = taste.write_digest
write_metrics = taste.write_metrics
_audit = taste._audit
_scope_for = taste._scope_for
_dimensions_for = taste._dimensions_for
apply_actions = taste.apply_actions
add_rule = taste.add_rule

_synth_committable = taste_mine._synth_committable
_cached_synth = taste_mine._cached_synth
_replayable_synth = taste_mine._replayable_synth
_cached_extract_progress = taste_mine._cached_extract_progress
_extraction_contract = taste_mine._extraction_contract
_session_source_keys = taste_mine._session_source_keys
_session_refs = taste_mine._session_refs
_resume_mismatch_reason = taste_mine._resume_mismatch_reason
_extract_batches = taste_mine._extract_batches

_preflight_apply_manifest = taste_apply._preflight_apply_manifest
_create_apply_safepoint = taste_apply._create_apply_safepoint
apply_from_manifest = taste_apply.apply_from_manifest

main = cli.main


if __name__ == "__main__":
    raise SystemExit(cli._dispatch())
