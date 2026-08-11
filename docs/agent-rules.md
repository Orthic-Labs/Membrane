# Adapt Rules

## Purpose
Adapt mines durable user-authored corrections from local sessions and proposes scoped preference records.
It never retrains models or stores private chain-of-thought.

## Canonical sources
- Read `README.md` for pipeline and authority rules.
- Read `docs/architecture.md` for components and flows.
- Treat reviewed manifests and Crypt records as runtime authority.

## Commands
- Run `python3 adapt.py --smoke` for a dry-run pipeline smoke.
- Run repository tests with the workspace tools venv (`.venv-tools`, provisioned by `tools/setup-workspace.py`), never a bare interpreter; it is the only environment pinning `scipy`/`numpy`.
- Run `python3 adapt.py doctor issue --out <receipt>` to issue conformance evidence.
- Run `python3 adapt.py doctor validate --receipt <receipt>` to validate it.

## Locked invariants
- Admit durable authority only from authenticated user-origin evidence.
- Quarantine assistant narration, echoed repository text, authority expansion, and security weakening.
- Keep standing preferences, scoped decisions, playbooks, and episodic facts distinct.
- Require immutable reviewed payload hashes before apply.
- Keep apply opt-in, transactional, journaled, integrity-checked, and reversible.
- Prevent standalone or offline stubs from performing live applies.
- Compile only root-scoped standing preferences into bounded always-on context.

## Verification
- Run dry-run smoke before any manifest apply.
- Validate source session identity, payload hash, scope, and canonical rule pool.
- Run database integrity checks after apply and rollback tests.
