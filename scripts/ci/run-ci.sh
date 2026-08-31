#!/usr/bin/env bash
set -euo pipefail

pnpm install --frozen-lockfile
pnpm --dir apps/membrane-hub install --frozen-lockfile
if [[ "${RIGHT_GIT_RUST_CHANGED:-true}" != "false" ]]; then
  cargo test --manifest-path engine/Cargo.toml --workspace --locked --no-fail-fast
else
  echo "Building test-required Membrane binaries from cache: right-git found no Rust-impacting changes."
  cargo build --manifest-path engine/Cargo.toml --locked --package membrane --bin membrane
  cargo build --manifest-path engine/Cargo.toml --locked --package membrane-runtime --example hub_runtime_test_host
fi
pnpm test
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
