#!/usr/bin/env bash
set -euo pipefail

pnpm install --frozen-lockfile
pnpm test
pnpm test:random
pnpm test:all
node scripts/ci/check-generated.mjs
node scripts/ci/check-network-boundary.mjs
