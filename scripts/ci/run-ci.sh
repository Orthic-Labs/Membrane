#!/usr/bin/env bash
set -euo pipefail

pnpm install --frozen-lockfile
pnpm --dir apps/membrane-hub install --frozen-lockfile

# One CI command, identical everywhere: a new script file must not pass locally
# by being invisible to a tracked-file enumeration and then fail in CI.
untracked_scripts="$(git ls-files --others --exclude-standard scripts apps/membrane-hub/scripts | grep -E '\.(mjs|cjs|ps1|py)$' || true)"
if [[ -n "$untracked_scripts" ]]; then
  echo "run-ci: untracked script files would pass locally and fail in CI:" >&2
  echo "$untracked_scripts" >&2
  exit 1
fi
if [[ "${RIGHT_GIT_RUST_CHANGED:-true}" != "false" ]]; then
  cargo test --manifest-path engine/Cargo.toml --workspace --locked --no-fail-fast
else
  echo "Building test-required Membrane binaries from cache: right-git found no Rust-impacting changes."
  cargo build --manifest-path engine/Cargo.toml --locked --package membrane --bin membrane
  cargo build --manifest-path engine/Cargo.toml --locked --package membrane-runtime --example hub_runtime_test_host
fi
pnpm test
RIGHT_RELEASE_OFFLINE=1 pnpm --dir apps/membrane-hub test
node --test scripts/release/*.test.mjs
pnpm test:random
node scripts/ci/check-release-identity.mjs
node scripts/ci/check-generated.mjs
node scripts/ci/check-network-boundary.mjs
node scripts/ci/check-lifecycle-conformance.mjs
node --test scripts/ci/check-adapt-ontology.test.mjs
node scripts/ci/check-adapt-ontology.mjs
node --test scripts/ci/check-runtime-language-manifest.test.mjs
node scripts/ci/check-runtime-language-manifest.mjs
node --test scripts/ci/check-invocation-graph.test.mjs
node scripts/ci/check-invocation-graph.mjs
node --test scripts/ci/check-native-contract-fixtures.test.mjs
node scripts/ci/check-native-contract-fixtures.mjs
