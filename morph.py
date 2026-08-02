#!/usr/bin/env python3
"""Orthic Morph CLI entry (display name). IDs and package paths stay `adapt*`."""
from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import cli  # noqa: E402


def main(argv: list[str] | None = None) -> int:
    return cli._dispatch(argv)


if __name__ == "__main__":
    raise SystemExit(main())
