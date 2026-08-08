# Fleet & replication view

`membrane-runtime::fleet::project_fleet` and Hub `fleet.mjs` expose a read-only projection of authoritative installation and replication observations. Every row carries explicit state, reason, source, evidence, and resolver. Missing observations remain `unknown`; no peer, liveness, health, lag, or closure row is synthesized.
