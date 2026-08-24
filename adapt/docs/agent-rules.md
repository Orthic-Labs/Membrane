# Adapt Rules

## Purpose
Adapt is Membrane's governed behavioral-learning subsystem. Taste proposes user-backed preferences; Insights proposes evidence-backed failure/gotcha records.
It never retrains models or stores private chain-of-thought.

## Canonical sources
- Read `../docs/subsystems/ADAPT_CANONICAL_PRODUCT_AND_ARCHITECTURE.md` for Adapt semantics, Taste/Insights authority, lifecycle, evaluation, and feature dependencies.
- Read `../migration/native-rust/MEMBRANE-NATIVE-RUST-MIGRATION-AND-CODERIGHT-INTEGRATION.md` for runtime/process cutover, packaging, deletion, and sequencing.
- Read `README.md` as a current implementation projection.
- Read `docs/architecture.md` for components and flows.
- Treat reviewed manifests and Cortex records as runtime authority.

## Commands
- Run product operations through native `membrane adapt`.
- Run native tests through `rightkit cargo test -p membrane-adapt -p membrane-transcript` from `engine/`.
- Run `membrane adapt doctor` for native invariant checks.
- Use Python only for release-excluded differential/evaluation tooling through workspace tools venv.

## Locked invariants
- Admit durable authority only from authenticated user-origin evidence.
- Keep Adapt proposal eligibility, Cortex durable admission, and Membrane context admission distinct.
- Quarantine assistant narration, echoed repository text, authority expansion, and security weakening.
- Keep Taste preferences, Insights findings, and non-Taste episodic facts distinct.
- Never let silent acceptance alone activate Taste; require authenticated human-act evidence for post-accept edits.
- Resolve authority/evidence class before specificity.
- Require immutable reviewed payload hashes before apply.
- Keep apply opt-in, transactional, journaled, and integrity-checked.
- Fail closed when required workspace services are unavailable.
- Compile only root-scoped standing preferences into bounded always-on context.
- Add no production Python/Node Adapt behavior; legacy Python is release-excluded differential evidence only.

## Verification
- Run dry-run smoke before any manifest apply.
- Validate source session identity, payload hash, scope, and canonical rule pool.
- Verify authenticated batch receipts after apply; do not expose compatibility reversal paths.
