# Soak reports

`scripts/run-soak.mjs` writes its machine-readable report here when
`--report <path>` points inside this directory. The contract test
`tests/soak-contract.test.mjs` asserts the report's invariants (no
corruption, all 9 fault classes applied, per-repo independent degradation).

Run a deterministic CI soak:

```sh
node scripts/run-soak.mjs --seed 1 --duration-events 500 --report evals/soak/ci-soak.json
```

Same seed produces a byte-for-byte reproducible report summary.

The envelope table for the soak itself lives in this directory's sibling
`evals/performance-envelopes.json` — the soak is a real-budget check, not a
separate budget table.
