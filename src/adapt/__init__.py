"""Adapt: durable, provenance-bound preference learning."""


def main(*args, **kwargs):
    """Run Adapt's library entrypoint without eager CLI imports."""
    from .cli import main as _main

    return _main(*args, **kwargs)


def apply_from_manifest(*args, **kwargs):
    """Apply one reviewed manifest through Adapt's guarded pipeline."""
    from .taste_apply import apply_from_manifest as _apply

    return _apply(*args, **kwargs)


__all__ = ["apply_from_manifest", "main"]
