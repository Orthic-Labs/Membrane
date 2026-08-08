# Release-channel compatibility

Channel values are stable (`stable`, `beta`, `nightly`) and support values are explicit (`supported`, `degraded`, `unsupported`, `unknown`). Consumers must fail closed on unknown schema compatibility. `migration_required` means migration must complete before activation; rollback restores the prior release and reverses migration where possible. Signed update evidence is required before any update may be considered available; source projections and hub rendering never mutate release state.

Hub displays required-update as `required`, `not_required`, or `unknown`; missing signed evidence remains unavailable and cannot imply either action.
