# Pull qualification entrypoint

Pull qualification runs deterministic candidate selection/admission against
frozen ContextCandidateSet fixtures, then checks eligibility, sufficiency,
headroom, budget, omission receipts, packet bounds, & publication determinism.

Implementation entrypoint: `membrane cli pull plan-context --candidate-set <fixture>`.
Federation qualification uses `membrane cli pull federate` with a fixture gateway.
