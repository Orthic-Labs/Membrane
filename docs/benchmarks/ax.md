# Behavioral AX (CX-B7)

CX-B7 is Cortex's Phase-D behavioral agent-experience harness: it runs the
scenario corpus in `evals/ax/scenarios` (12 JSON-compatible YAML files, per
the AX audit standard §6.7) repeatedly and reports `pass^k` per §7.2. It is
the executable complement to the static conformance suite (`evals/ax/
run-conformance.mjs`, Phase C), which runs in CI under the `ax-conformance`
job of `.github/workflows/qualification.yml`.

## Command

```text
node evals/ax/run-behavioral.mjs --driver stub --k 3 --root .
node evals/ax/run-behavioral.mjs --driver claude --k 5 --root <repo>
```

| Flag | Meaning |
|---|---|
| `--driver` | `stub` (default) or `claude`. `claude` is never the default and is rejected when `CI` is set. |
| `--k` | independent trials per scenario (default 3). The report always carries `pass^1`, `pass^3`, `pass^5`. |
| `--root` | repository root containing `evals/ax/scenarios` (default `.`). |
| `--wait-seconds` | how long to wait for scenario files to appear (default 120); used when the corpus is landing in a parallel lane. |

The runner loads `evals/ax/scenarios/*.yaml` (JSON-compatible YAML: the stub
parses the files as JSON with no YAML dependency), runs each scenario `k`
times, and delegates report writing to `evals/ax/report.mjs`, which produces
`.audit/cx-b7/ax-report.json` and `.audit/cx-b7/ax-report.md` plus per-run
transcripts under `.audit/cx-b7/alchemist/behavioral/`.

## Conformance proof boundary

Conformance and behavioral evaluation prove different things:

- **Conformance (Phase C, CI):** proves the advertised MCP surface is
  callable with schema-valid inputs, rejects invalid inputs with typed stable
  errors, and returns no raw stack traces. It does not prove any agent can
  choose and chain those tools to complete a realistic goal.
- **Behavioral (Phase D):** proves that a driver completes each scenario to a
  verified final environment and makes only claims the evidence supports.
  What it proves depends entirely on the driver (next section).

Neither phase proves product quality of the surrounding agent experience by
itself; the full claim requires the frozen scenario corpus, an executed
real-agent matrix, and claim reconciliation (§6.4 Phase F).

## Stub is not a proof

The `stub` driver is deterministic, scripted, and **harness validation only**.
It executes each scenario's declared branch; across the corpus those branches
cover happy paths, failures, recovery, and refusal. It then verifies the
expected final environment **before** admitting any claim. A
stub pass proves the harness loads all 12 scenarios, runs each one `k` times,
keeps skipped-pending separate from passes, and enforces
final-environment-before-claims. It does **not** prove that a real agent can
achieve the scenario goals: the stub never sees tool outputs, never makes a
model decision, and cannot produce an unsupported claim. Stub runs are safe
for CI and the default path performs **no network calls and no model calls**
(asserted in the report's `defaultPathNoModelOrNetworkCalls` check).

## Real-agent matrix

`--driver claude` shells out to the local `claude -p` CLI. It requires
explicit invocation, is rejected whenever `CI` is set
(`claude_driver_forbidden_in_ci`), and exits typed with
`claude_cli_unavailable` (exit code 2) when the CLI is not installed. It is
never wired into CI.

The intended real-agent matrix, per §6.6 `ax.behavior.consistency`, is at
least two distinct agents/models, each cell running ≥5 independent trials per
scenario with the final environment verified before claims. The matrix is
currently **unexecuted**: no real-agent cell exists in any committed report,
so every claim that depends on it remains `UNPROVEN` (banner below).

## UNPROVEN banner and retirement condition

Every CX-B7 report (JSON and Markdown) carries an explicit `UNPROVEN` banner:
end-to-end behavioral AX is formally unproven until a real-agent matrix has
been executed. The exact retirement condition is embedded in
`evals/ax/report.mjs` and printed in every report:

1. Every scenario in `evals/ax/scenarios` (currently 12) executed by at least
   one real agent via `--driver claude` (never in CI).
2. The executed matrix covers at least two distinct real agents/models.
3. Each matrix cell ran at least 5 independent trials per scenario with a
   verified final environment before any claim was admitted.
4. `pass^1`/`pass^3`/`pass^5` meet the release thresholds the owner sets at
   that time, with skipped-pending reported separately.
5. Claim-fidelity checks (forbidden claims, forbidden reason codes, forbidden
   operations) passed for every executed cell.
6. The report records the exact commit, environment, driver version, and
   agent/model versions that produced it.

Removing the banner without that evidence is an unsupported claim and fails
the claim-fidelity gate.

## CX-B5 CI ownership

The behavioral harness has **no CI ownership**. The stub driver stays
advisory and is not wired into `.github/workflows/qualification.yml`; the only
CX-B7 CI job is `ax-conformance`, which runs `run-conformance.mjs` (Phase C,
deterministic contract checks). The `claude` driver is additionally
hard-blocked from CI. CI ownership of CX-B7 therefore means: conformance is
enforced in CI, behavioral evidence is produced out-of-band (local or
scheduled owner-invoked runs) and committed as reports — the same pattern the
implementation plan (W4, `solimplement.md`) describes
as "Phase D is one command away" rather than a CI gate.

## Reading a report

`ax-report.json` carries, per scenario, every trial with its status, the
verified final environment, `envVerified`/`claimsChecked` flags, and any
failures; plus `pass^1`/`pass^3`/`pass^5` over executed scenarios (skipped-
 pending scenarios never enter pass^k numerators or denominators) and the
`UNPROVEN` banner. `ax-report.md` is the same data as a human summary:
per-scenario status table, pass^k table, checks, and the retirement condition.
