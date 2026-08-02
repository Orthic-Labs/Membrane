"""Taste mining helpers — extract/synth orchestration support."""
from __future__ import annotations

import hashlib
import sys
from collections import defaultdict
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import adapt_llm  # noqa: E402
import adapt_sessions as ts  # noqa: E402
import outcomes  # noqa: E402

def _synth_committable(outcome: str) -> bool:
    return outcome in outcomes.COMMITTABLE


def _cached_synth(journal, batch_id: str) -> tuple[str, list[dict]]:
    record = journal.cached_payload(batch_id, "synthesized")
    if record is None:
        return outcomes.Outcome.PROVIDER_FAILED, []
    return record.get("synth_outcome", outcomes.Outcome.PROVIDER_FAILED), list(
        record.get("actions", [])
    )


def _replayable_synth(journal, batch_id: str) -> tuple[bool, str, list[dict]]:
    outcome, actions = _cached_synth(journal, batch_id)
    return _synth_committable(outcome), outcome, actions


def _cached_extract_progress(journal, batch_id: str) -> tuple[int, list[dict]]:
    latest_by_index: dict[int, dict] = {}
    for entry in journal.batches():
        if entry.get("batch_id") != batch_id or entry.get("stage") != "extracted":
            continue
        index = int(entry.get("batch", 0))
        if index > 0:
            latest_by_index[index] = entry

    completed_batch = 0
    observations: list[dict] = []
    for index in range(1, max(latest_by_index, default=0) + 1):
        entry = latest_by_index.get(index)
        if entry is None or not (
            "observations" in entry or entry.get("valid_empty") is True
        ):
            break
        if "observations" in entry:
            observations = list(entry["observations"])
        completed_batch = index
    return completed_batch, observations


def _extraction_contract() -> dict:
    """Stable fingerprint for deciding whether cached extraction is reusable."""
    return {
        "version": 1,
        "batch_char_budget": adapt_llm.BATCH_CHAR_BUDGET,
        "max_tokens": adapt_llm.MAX_TOKENS,
        "extract_prompt_sha256": hashlib.sha256(
            adapt_llm.EXTRACT_SYSTEM.encode("utf-8")
        ).hexdigest(),
        "synth_prompt_sha256": hashlib.sha256(
            adapt_llm.SYNTH_SYSTEM.encode("utf-8")
        ).hexdigest(),
        "synth_max_tokens": adapt_llm.SYNTH_MAX_TOKENS,
        "preference_prefilter_version": ts.PREFERENCE_PREFILTER_VERSION,
    }


def _session_source_keys(sessions) -> list[str]:
    """Keep ordinary IDs stable while disambiguating client-side collisions."""
    counts: dict[str, int] = defaultdict(int)
    for session in sessions:
        counts[session.session_id] += 1
    keys = [
        session.session_id
        if counts[session.session_id] == 1
        else f"{session.session_id}\0{session.tool}\0{ts.state_key(session.tool, session.path)}"
        for session in sessions
    ]
    if len(keys) != len(set(keys)):
        raise ValueError("client session identity remains duplicated after state-key binding")
    return keys


def _session_refs(sessions) -> list[dict]:
    source_keys = _session_source_keys(sessions)
    return [{
        "session_id": session.session_id,
        "source_key": source_key,
        "tool": session.tool,
        "path_stem": session.path.stem,
        "mtime": session.mtime,
    } for session, source_key in zip(sessions, source_keys)]


def _resume_mismatch_reason(discovered: dict, current_session_refs: list[dict]
                            ) -> str | None:
    cached_refs = discovered.get("session_refs")
    if cached_refs != current_session_refs:
        return "cached session identity does not match current discovery"
    if discovered.get("extraction_contract") != _extraction_contract():
        return "cached extraction contract does not match current extractor"
    return None


def _extract_batches(batches: list[list[tuple[str, str, str]]], *, lane: str,
                     journal, batch_id: str, observations: list[dict],
                     completed_batch: int, quiet: bool, workers: int
                     ) -> tuple[list[dict], list[outcomes.BatchOutcome],
                                tuple[int, outcomes.BatchOutcome] | None]:
    """Extract independent batches concurrently, checkpointing in order."""
    batch_outcomes: list[outcomes.BatchOutcome] = []
    pending = [
        (index, batch) for index, batch in enumerate(batches, 1)
        if index > completed_batch
    ]
    for offset in range(0, len(pending), workers):
        window = pending[offset:offset + workers]
        if not quiet:
            for index, batch in window:
                print(f"  extract batch {index}: {len(batch)} turns")
        if len(window) == 1:
            index, batch = window[0]
            results = {index: adapt_llm.extract_observations(batch, lane=lane)}
        else:
            with ThreadPoolExecutor(max_workers=len(window)) as pool:
                futures = {
                    index: pool.submit(adapt_llm.extract_observations, batch, lane=lane)
                    for index, batch in window
                }
                results = {}
                for index, future in futures.items():
                    try:
                        results[index] = future.result()
                    except Exception as exc:
                        results[index] = outcomes.BatchOutcome.provider_failed(
                            f"{type(exc).__name__}: {exc}"
                        )
        for index, _batch in window:
            batch_outcome = results[index]
            batch_outcomes.append(batch_outcome)
            receipt = batch_outcome.provider_receipt()
            if batch_outcome.outcome == outcomes.Outcome.SUCCESS:
                observations.extend(batch_outcome.actions)
                if journal:
                    journal.record(batch_id, "extracted", batch=index,
                                   observations=observations, **receipt)
            elif batch_outcome.outcome == outcomes.Outcome.VALID_EMPTY:
                if journal:
                    journal.record(batch_id, "extracted", batch=index,
                                   valid_empty=True, observations=observations,
                                   **receipt)
            else:
                if journal:
                    journal.record(batch_id, "extracted", batch=index,
                                   outcome=batch_outcome.outcome,
                                   reason=batch_outcome.reason, **receipt)
                return observations, batch_outcomes, (index, batch_outcome)
    return observations, batch_outcomes, None
