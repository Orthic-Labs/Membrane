# D19: container image for CI/headless use only — not a desktop distribution.
FROM node:22-bookworm-slim
WORKDIR /opt/cortex
COPY package.json pnpm-lock.yaml ./
RUN corepack enable && pnpm install --frozen-lockfile --prod
COPY scripts scripts
COPY src src
COPY watchman watchman
COPY schemas schemas
COPY references references
COPY README.md LICENSE docs/CHANGELOG.md ./
ENV CORTEX_NO_UPDATE_CHECK=1
ENTRYPOINT ["node", "scripts/cortex.mjs"]
