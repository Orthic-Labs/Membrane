"""Morph entrypoint: direct transcript Taste v2 & reviewed-manifest apply."""
from __future__ import annotations
import importlib
import sys
from pathlib import Path
sys.path.insert(0, str(Path(__file__).resolve().parent))
import cli
import taste_apply

main = cli.main
apply_from_manifest = taste_apply.apply_from_manifest

_LEGACY = {"taste", "ts", "_legacy_miner", "morph_llm", "outcomes", "admission", "authority",
           "preference_record", "cross_machine", "run_journal", "manifest", "rollback", "morph_persistence", "core_compiler"}
def __getattr__(name: str):
    """Compatibility facade: legacy modules load only on explicit historical access."""
    if name.startswith("_") or name in {"apply_actions", "RULES_FILE", "DIGEST_FILE", "WORKSPACE_ROOT", "preflight_apply", "initialized", "rule_body", "load_rules", "save_rules", "write_digest", "write_metrics", "add_rule"}:
        module = importlib.import_module("taste_mine") if name in {"_synth_committable", "_cached_synth", "_replayable_synth", "_cached_extract_progress", "_extraction_contract", "_session_refs", "_resume_mismatch_reason", "_extract_batches"} else importlib.import_module("taste")
        if name == "ts": value = importlib.import_module("morph_sessions")
        else: value = getattr(module, name)
        globals()[name] = value
        return value
    if name not in _LEGACY: raise AttributeError(name)
    module_name = {"ts": "morph_sessions", "_legacy_miner": "taste_mine"}.get(name, name)
    value = importlib.import_module(module_name)
    globals()[name] = value
    return value

if __name__ == "__main__": raise SystemExit(cli._dispatch())
