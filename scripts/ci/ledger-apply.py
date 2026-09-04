#!/usr/bin/env python3
"""Materialize reviewed edits on the dedicated branch, with exact blob guards.

Only declared file edits, Cargo lock resolution, and canon index generation are
supported. No build/test binaries, installation, activation, release or forced
ref update is performed. This authoring bridge is removed after integration.
"""
import hashlib
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


def blob(data):
    return hashlib.sha1(b"blob " + str(len(data)).encode() + b"\0" + data).hexdigest()


def main():
    if os.environ.get("GITHUB_REF") != "refs/heads/" + BRANCH:
        raise SystemExit("refused: materialization is restricted to the Ledger branch")
    batch = json.loads(BATCH.read_text(encoding="utf-8"))
    message = batch["message"]
    if not isinstance(message, str) or not message.strip() or len(message) > 4000:
        raise SystemExit("invalid commit message")
    pending = {}
    for edit in batch["files"]:
        relative = edit["path"]
        path = ROOT / relative
        if (not relative.startswith(ALLOWED) or ".." in Path(relative).parts
                or path.is_symlink() or not path.resolve().is_relative_to(ROOT)):
            raise SystemExit("refused edit path: " + relative)
        if relative in pending:
            raise SystemExit("duplicate edit path: " + relative)
        data = path.read_bytes() if path.exists() else None
        expected = edit.get("sha")
        if (data is None) != (expected is None) or (data is not None and blob(data) != expected):
            raise SystemExit("source hash drift: " + relative)
        text = data.decode("utf-8") if data is not None else ""
        if "content" in edit:
            text = edit["content"]
        for change in edit.get("replacements", []):
            before, after = change["before"], change["after"]
            count = change.get("count", 1)
            if not before or text.count(before) != count:
                raise SystemExit("replacement ambiguity in " + relative + ": " + before[:80])
            text = text.replace(before, after)
        for region in edit.get("regions", []):
            start, end = region["start"], region["end"]
            if not start or not end or text.count(start) != 1 or text.count(end) != 1:
                raise SystemExit("region ambiguity in " + relative)
            first, last = text.index(start), text.index(end)
            if first >= last:
                raise SystemExit("region order invalid in " + relative)
            text = text[:first] + region["content"] + text[last:]
        pending[relative] = (data, text.encode("utf-8"))
    for relative, (old, _) in pending.items():
        path = ROOT / relative
        if (path.read_bytes() if path.exists() else None) != old:
            raise SystemExit("source changed while preparing: " + relative)
    for relative, (_, new) in pending.items():
        path = ROOT / relative
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(new)
    extra = []
    if batch.get("refresh_lock"):
        # Metadata resolves the existing lockfile incrementally; it compiles
        # nothing and does not upgrade unrelated dependencies deliberately.
        subprocess.run(["cargo", "metadata", "--manifest-path", "engine/Cargo.toml",
                        "--format-version", "1", "--no-deps"], cwd=ROOT,
                       stdout=subprocess.DEVNULL, check=True)
        extra.append("engine/Cargo.lock")
    BATCH.unlink()
    git("config", "user.name", "github-actions[bot]")
    git("config", "user.email", "41898282+github-actions[bot]@users.noreply.github.com")
    git("add", "--", *pending, *extra, "scripts/ci/ledger-edits.json")
    if batch.get("regenerate_canons"):
        subprocess.run(["node", "scripts/ci/check-atomic-canons.mjs", "--write"], cwd=ROOT, check=True)
        git("add", "--", "docs/canon/README.md", "docs/pending/README.md")
    git("diff", "--cached", "--check")
    git("commit", "-m", message + "\n\n[skip ci]")
    git("push", "origin", "HEAD:refs/heads/" + BRANCH)
    print("Materialized revision: " + git("rev-parse", "HEAD"))
    print("Changed files: " + ", ".join(pending))


if __name__ == "__main__":
    main()
