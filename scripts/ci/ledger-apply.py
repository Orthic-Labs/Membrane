#!/usr/bin/env python3
"""Materialize reviewed edits on the dedicated branch, with exact blob guards.

Only declared file edits, Cargo lock resolution, and canon index generation are
supported. No build/test binaries, installation, activation, release or forced
ref update is performed. This authoring bridge is removed after integration.
"""
import base64
import gzip
import hashlib
import json
import os
import tempfile
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


def validate_edit_path(relative):
    path = ROOT / relative
    if (not relative.startswith(ALLOWED) or ".." in Path(relative).parts
            or path.is_symlink() or not path.resolve().is_relative_to(ROOT)):
        raise SystemExit("refused edit path: " + relative)
    return path


def apply_reviewed_patch(batch):
    encoded = batch.get("patch_gzip_base64")
    relative_patch = batch.get("patch")
    temporary = None
    if encoded is not None:
        if not isinstance(encoded, str) or len(encoded) > 512_000:
            raise SystemExit("invalid compressed reviewed patch")
        try:
            patch_bytes = gzip.decompress(base64.b64decode(encoded, validate=True))
        except Exception as error:
            raise SystemExit("invalid compressed reviewed patch") from error
        if len(patch_bytes) > 2_000_000:
            raise SystemExit("reviewed patch exceeds bounded size")
        temporary = tempfile.NamedTemporaryFile(prefix="ledger-edits-", suffix=".patch", delete=False)
        temporary.write(patch_bytes)
        temporary.close()
        patch_path = Path(temporary.name)
        relative_patch = None
    else:
        if not isinstance(relative_patch, str) or not relative_patch.startswith("scripts/ci/"):
            raise SystemExit("invalid reviewed patch path")
        patch_path = ROOT / relative_patch
        if not patch_path.is_file() or patch_path.is_symlink():
            raise SystemExit("reviewed patch missing or unsafe")
    base_blobs = batch.get("base_blobs")
    if not isinstance(base_blobs, dict) or not base_blobs:
        raise SystemExit("reviewed patch requires exact base blobs")
    touched = []
    for relative, expected in sorted(base_blobs.items()):
        path = validate_edit_path(relative)
        if not isinstance(expected, str) or len(expected) != 40:
            raise SystemExit("invalid base blob for " + relative)
        if not path.is_file():
            raise SystemExit("base file missing: " + relative)
        observed = git("hash-object", "--", relative)
        if observed != expected:
            raise SystemExit("source hash drift: " + relative)
        touched.append(relative)
    try:
        subprocess.run(["git", "apply", "--check", "--whitespace=error-all", str(patch_path)], cwd=ROOT, check=True)
        subprocess.run(["git", "apply", "--whitespace=error-all", str(patch_path)], cwd=ROOT, check=True)
    finally:
        if temporary is not None:
            patch_path.unlink(missing_ok=True)
    changed = set(git("diff", "--name-only").splitlines())
    expected_changed = set(touched)
    if changed != expected_changed:
        raise SystemExit("reviewed patch changed unexpected paths: " + repr(sorted(changed ^ expected_changed)))
    if relative_patch is not None:
        (ROOT / relative_patch).unlink()
    return touched, relative_patch


def main():
    if os.environ.get("GITHUB_REF") != "refs/heads/" + BRANCH:
        raise SystemExit("refused: materialization is restricted to the Ledger branch")
    batch = json.loads(BATCH.read_text(encoding="utf-8"))
    message = batch["message"]
    if not isinstance(message, str) or not message.strip() or len(message) > 4000:
        raise SystemExit("invalid commit message")
    patch_touched = []
    patch_path = None
    if batch.get("patch") is not None or batch.get("patch_gzip_base64") is not None:
        patch_touched, patch_path = apply_reviewed_patch(batch)
    pending = {}
    for edit in batch.get("files", []):
        relative = edit["path"]
        path = validate_edit_path(relative)
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
        subprocess.run(["cargo", "metadata", "--manifest-path", "engine/Cargo.toml", "--format-version", "1", "--no-deps"], cwd=ROOT, stdout=subprocess.DEVNULL, check=True)
        extra.append("engine/Cargo.lock")
    BATCH.unlink()
    git("config", "user.name", "github-actions[bot]")
    git("config", "user.email", "41898282+github-actions[bot]@users.noreply.github.com")
    paths_to_stage = list(pending) + patch_touched + extra + ["scripts/ci/ledger-edits.json"]
    if patch_path is not None:
        paths_to_stage.append(patch_path)
    git("add", "-A", "--", *paths_to_stage)
    if batch.get("regenerate_canons"):
        subprocess.run(["node", "scripts/ci/check-atomic-canons.mjs", "--write"], cwd=ROOT, check=True)
        git("add", "--", "docs/canon/README.md", "docs/pending/README.md")
    git("diff", "--cached", "--check")
    git("commit", "-m", message + "\n\n[skip ci]")
    git("push", "origin", "HEAD:refs/heads/" + BRANCH)
    print("Materialized revision: " + git("rev-parse", "HEAD"))
    changed = patch_touched + list(pending)
    print("Changed files: " + ", ".join(changed))


if __name__ == "__main__":
    main()
