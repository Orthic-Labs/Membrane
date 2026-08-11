"""Run Adapt with ``python -m adapt``."""

from .cli import _dispatch


if __name__ == "__main__":
    raise SystemExit(_dispatch())
