"""Source-checkout entrypoint for Adapt."""
from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent / "src"))

from adapt import cli, taste_apply  # noqa: E402

main = cli.main
apply_from_manifest = taste_apply.apply_from_manifest


if __name__ == "__main__":
    raise SystemExit(cli._dispatch())
