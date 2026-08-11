from __future__ import annotations

import importlib.util
from pathlib import Path


SCRIPT = Path(__file__).resolve().parent / "eval" / "evaluate_full_treatment_census.py"

import pytest

# Same portability contract as test_delivery_parity.py: the census reads the
# machine-local frozen evidence pack under .cache/ — skip with a named reason
# on checkouts without it instead of failing.
import sys

sys.path.insert(0, str(Path(__file__).resolve().parent))
import workspace_runtime  # noqa: E402

_FROZEN_TREATMENT = workspace_runtime.workspace_root() / ".cache/adapt-delivery-parity/full/frozen/adapt-treatment.json"
requires_frozen_treatment = pytest.mark.skipif(
    not _FROZEN_TREATMENT.exists(),
    reason="machine-local frozen delivery-parity treatment absent (.cache/adapt-delivery-parity/full/frozen)",
)


def _load():
    spec = importlib.util.spec_from_file_location("full_treatment_census", SCRIPT)
    assert spec and spec.loader
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


@requires_frozen_treatment
def test_actual_census_covers_every_taste_and_adapt_record():
    module = _load()
    taste_rules, adapt_records = module.load_sources(
        module.DEFAULT_TASTE, module.DEFAULT_TREATMENT
    )
    taste_cases = module.build_taste_cases(taste_rules)
    adapt_cases = module.evidence.build_cases(adapt_records)
    taste_facts = module.load_taste_artifact_facts(module.DEFAULT_TASTE)
    assert len(taste_rules) == len(taste_cases) == 52
    assert len({rule.category for rule in taste_rules}) == 18
    assert taste_facts == {
        "product_reported_categories": 32,
        "artifact_heading_occurrences": 34,
        "artifact_unique_headings": 18,
    }
    assert len(adapt_records) == 57
    assert sum(row["kind"] == "rule_self" for row in adapt_cases) == 57
