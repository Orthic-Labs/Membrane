#!/bin/sh
# Rebind the Blueprint graph to the current commit.
#
# The sealed generation in .agent/graph/graph.db is bound to the commit it was
# built at, so every new commit leaves it behind HEAD and the federation lane
# degrades `blueprint_stale` on every prompt until a rebuild runs. This script
# is the repo-revision trigger for that rebuild.
#
# Everything it writes (.agent/, .blueprint/, and the two generated human docs)
# is gitignored, so the rebuild never dirties the tree and never re-triggers
# itself. Failures are non-fatal: a commit must never be blocked by indexing.
set -eu

repo_root=$(git rev-parse --show-toplevel 2>/dev/null) || exit 0
cd "$repo_root" || exit 0

command -v blueprint >/dev/null 2>&1 || {
    echo "blueprint-refresh: blueprint not on PATH; skipping" >&2
    exit 0
}

blueprint build >/dev/null 2>&1 || {
    echo "blueprint-refresh: build failed; graph left at its previous generation" >&2
    exit 0
}

head=$(git rev-parse HEAD 2>/dev/null || echo unknown)
echo "blueprint-refresh: graph rebound to ${head}"
