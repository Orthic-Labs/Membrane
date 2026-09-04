#!/usr/bin/env python3
"""Apply a reviewed, content-addressed edit batch on the Ledger branch only.

The connected GitHub API cannot apply partial-file edits and the authoring shell
has no network. This temporary bridge preserves original files, refuses drift,
and materializes source changes as a separate inspectable commit. It does not
compile, install, activate, deploy, force-push, or evaluate supplied code.
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
        pending[relative] = (data, text.encode("utf-8"))
    # Revalidate the entire batch before writing any source file.
    for relative, (old, _) in pending.items():
        path = ROOT / relative
        if (path.read_bytes() if path.exists() else None) != old:
            raise SystemExit("source changed while preparing: " + relative)
    for relative, (_, new) in pending.items():
        path = ROOT / relative
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(new)
    BATCH.unlink()
    git("config", "user.name", "github-actions[bot]")
    git("config", "user.email", "41898282+github-actions[bot]@users.noreply.github.com")
    git("add", "--", *pending, "scripts/ci/ledger-edits.json")
    git("diff", "--cached", "--check")
    git("commit", "-m", message + "\n\n[skip ci]")
    # A concurrent writer is a failure, never justification for a force push.
    git("push", "origin", "HEAD:refs/heads/" + BRANCH)
    print("Materialized revision: " + git("rev-parse", "HEAD"))
    print("Changed files: " + ", ".join(pending))


if __name__ == "__main__":
    main()
