"""Morph entrypoint: direct transcript Taste v2 & reviewed-manifest apply."""
from __future__ import annotations

import sys
import importlib
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import cli  # noqa: E402
import taste_apply  # noqa: E402

# Compatibility reader facade.  The executable CLI above never routes mining
# through these modules; historical callers/manifests retain their public API.
taste = importlib.import_module("taste")
ts = importlib.import_module("morph" + "_sessions")
_legacy_miner = importlib.import_module("taste" + "_mine")
morph_llm = importlib.import_module("morph" + "_llm")
outcomes = importlib.import_module("outcomes")
admission = importlib.import_module("admission")
authority = importlib.import_module("authority")
preference_record = importlib.import_module("preference_record")
cross_machine = importlib.import_module("cross_machine")
run_journal = importlib.import_module("run_journal")
manifest = importlib.import_module("manifest")
rollback = importlib.import_module("rollback")
morph_persistence = importlib.import_module("morph_persistence")
core_compiler = importlib.import_module("core_compiler")

RULES_FILE = taste.RULES_FILE
DIGEST_FILE = taste.DIGEST_FILE
WORKSPACE_ROOT = taste.WORKSPACE_ROOT
CRYPT_MUTATION_TIMEOUT_SECONDS = taste.CRYPT_MUTATION_TIMEOUT_SECONDS
_run_crypt = taste._run_crypt
preflight_apply = taste.preflight_apply
initialized = taste.initialized
rule_body = taste.rule_body
load_rules = taste.load_rules
save_rules = taste.save_rules
write_digest = taste.write_digest
write_metrics = taste.write_metrics
_audit = taste._audit
apply_actions = taste.apply_actions
add_rule = taste.add_rule
_synth_committable = _legacy_miner._synth_committable
_cached_synth = _legacy_miner._cached_synth
_replayable_synth = _legacy_miner._replayable_synth
_cached_extract_progress = _legacy_miner._cached_extract_progress
_extraction_contract = _legacy_miner._extraction_contract
_session_source_keys = _legacy_miner._session_source_keys
_session_refs = _legacy_miner._session_refs
_resume_mismatch_reason = _legacy_miner._resume_mismatch_reason
_extract_batches = _legacy_miner._extract_batches
_qualified_session_sources = taste._qualified_session_sources
_multiwriter_context = taste._multiwriter_context
_scope_for = taste._scope_for
_dimensions_for = taste._dimensions_for
_preflight_apply_manifest = taste_apply._preflight_apply_manifest
_create_apply_safepoint = taste_apply._create_apply_safepoint

main = cli.main
apply_from_manifest = taste_apply.apply_from_manifest

if __name__ == "__main__":
    raise SystemExit(cli._dispatch())
