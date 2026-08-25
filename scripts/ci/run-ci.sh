#!/usr/bin/env bash
set -euo pipefail

pnpm install --frozen-lockfile
# Temporary verification-branch gate: execute the Blueprint lease production
# path before unrelated baseline failures stop the broader repository suite.
node --test \
  blueprint/tests/store-lease.test.mjs \
  blueprint/tests/freshness-observation.test.mjs \
  blueprint/tests/production-store-lease.test.mjs \
  blueprint/tests/application-service.test.mjs
pnpm test
pnpm test:random
pnpm test:all
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
