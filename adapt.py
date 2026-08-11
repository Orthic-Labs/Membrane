"""Adapt entrypoint: provenance-bound Taste & reviewed-manifest apply."""
from __future__ import annotations
import sys
from pathlib import Path
sys.path.insert(0, str(Path(__file__).resolve().parent))
import cli
import taste_apply

main = cli.main
apply_from_manifest = taste_apply.apply_from_manifest

if __name__ == "__main__": raise SystemExit(cli._dispatch())
