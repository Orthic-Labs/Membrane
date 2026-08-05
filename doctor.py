#!/usr/bin/env python3
"""Orthic Morph Doctor — expose multiwriter conformance with honest scope.

Doctor today wraps the existing Morph multi-installation conformance receipt
surface (``multiwriter_conformance``): installation identity, canonical pool,
implementation/test hashes, Crypt service probe, transcript discovery
counts, and the append-only mirror boundary.

Net-new / not-yet (do not pretend these exist):
  - Cortex graph/claim health checks
  - Sentinel receipt / e2e wiring checks
  - Cross-system Doctor that assumes receipts those systems do not emit
"""
from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path
from typing import Sequence

sys.path.insert(0, str(Path(__file__).resolve().parent))

import multiwriter_conformance  # noqa: E402


SCOPE = {
    "product": "Orthic Morph Doctor",
    "implemented": [
        "multiwriter_conformance issue",
        "multiwriter_conformance validate",
    ],
    "not_yet": [
        "Cortex graph/claim health",
        "Sentinel receipt / e2e wiring",
        "cross-system Doctor assuming Cortex/Sentinel receipts",
    ],
}


def main(argv: Sequence[str] | None = None) -> int:
    parser = argparse.ArgumentParser(
        description=(
            "Orthic Morph Doctor — multiwriter conformance only. "
            "Cortex/Sentinel checks are not-yet."
        )
    )
    parser.add_argument(
        "--scope",
        action="store_true",
        help="print implemented vs not-yet Doctor surface as JSON and exit",
    )
    parser.add_argument(
        "conformance_args",
        nargs=argparse.REMAINDER,
        help="forwarded to multiwriter_conformance (issue|validate ...)",
    )
    args = parser.parse_args(list(argv) if argv is not None else None)
    if args.scope:
        print(json.dumps(SCOPE, indent=2, sort_keys=True))
        return 0
    forwarded = list(args.conformance_args)
    if forwarded and forwarded[0] == "--":
        forwarded = forwarded[1:]
    if not forwarded:
        print(json.dumps(SCOPE, indent=2, sort_keys=True))
        print(
            "usage: morph doctor issue --out RECEIPT.json\n"
            "       morph doctor validate --receipt RECEIPT.json\n"
            "       morph doctor --scope",
            file=sys.stderr,
        )
        return 2
    return multiwriter_conformance.main(forwarded)


if __name__ == "__main__":
    raise SystemExit(main())
