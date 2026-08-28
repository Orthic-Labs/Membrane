#!/usr/bin/env bash
set -euo pipefail

pnpm install --frozen-lockfile
pnpm test
pnpm test:random
cargo test --manifest-path engine/Cargo.toml --workspace --locked --no-fail-fast
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
