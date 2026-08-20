"""Adapt: durable, provenance-bound preference learning."""

from pathlib import Path as _Path
import sys as _sys

# Source checkouts expose Membrane-owned continuity beside Adapt. Keep this
# bootstrap local to source execution; installed packages provide normal
# package metadata instead.
_MEMBRANE_ROOT = _Path(__file__).resolve().parents[3]
if str(_MEMBRANE_ROOT) not in _sys.path:
    _sys.path.insert(0, str(_MEMBRANE_ROOT))


def main(*args, **kwargs):
    """Run Adapt's library entrypoint without eager CLI imports."""
    from .cli import main as _main

    return _main(*args, **kwargs)


def apply_from_manifest(*args, **kwargs):
    """Apply one reviewed manifest through Adapt's guarded pipeline."""
    from .taste_apply import apply_from_manifest as _apply

    return _apply(*args, **kwargs)


__all__ = ["apply_from_manifest", "main"]
