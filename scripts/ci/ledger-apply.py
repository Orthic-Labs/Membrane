#!/usr/bin/env python3
"""Apply one reviewed, hash-guarded Ledger patch on the dedicated branch."""
import json
import os
from pathlib import Path
import subprocess

ROOT = Path(__file__).resolve().parents[2]
BATCH = ROOT / "scripts/ci/ledger-edits.json"
BRANCH = "ledger-end-to-end"
ALLOWED = ("engine/", "mcp/", "docs/", "schemas/", "tests/", "scripts/tools/productization/")

def git(*args):
    return subprocess.check_output(["git", *args], cwd=ROOT, text=True).strip()

def main():
    if os.environ.get("GITHUB_REF") != "refs/heads/" + BRANCH:
        raise SystemExit("refused: wrong branch")
    batch = json.loads(BATCH.read_text(encoding="utf-8"))
    message = batch.get("message")
    patch = ROOT / batch.get("patch", "")
    if not isinstance(message, str) or not message.strip() or not patch.is_file() or patch.is_symlink():
        raise SystemExit("invalid reviewed patch batch")
    base = batch.get("base_blobs")
    if not isinstance(base, dict) or not base:
        raise SystemExit("reviewed patch requires base blobs")
    touched = []
    for relative, expected in sorted(base.items()):
        path = ROOT / relative
        if (not relative.startswith(ALLOWED) or ".." in Path(relative).parts or path.is_symlink()
                or not path.resolve().is_relative_to(ROOT) or not path.is_file()):
            raise SystemExit("refused edit path: " + relative)
        if git("hash-object", "--", relative) != expected:
            raise SystemExit("source hash drift: " + relative)
        touched.append(relative)
    subprocess.run(["git", "apply", "--check", "--whitespace=error-all", str(patch)], cwd=ROOT, check=True)
    subprocess.run(["git", "apply", "--whitespace=error-all", str(patch)], cwd=ROOT, check=True)
    changed = set(git("diff", "--name-only").splitlines())
    if changed != set(touched):
        raise SystemExit("reviewed patch changed unexpected paths")
    BATCH.unlink()
    patch.unlink()
    git("config", "user.name", "github-actions[bot]")
    git("config", "user.email", "41898282+github-actions[bot]@users.noreply.github.com")
    git("add", "-A", "--", *touched, "scripts/ci/ledger-edits.json")
    git("diff", "--cached", "--check")
    git("commit", "-m", message + "\n\n[skip ci]")
    git("push", "origin", "HEAD:refs/heads/" + BRANCH)
    print("Materialized revision: " + git("rev-parse", "HEAD"))
    print("Changed files: " + ", ".join(touched))

if __name__ == "__main__":
    main()
