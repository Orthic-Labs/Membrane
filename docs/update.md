# Transactional update

`membrane::update` requires a finite supervisor quiesce, verified staging, atomic
directory activation, schema migration, & atomic receipt publication. Cancellation
or any deterministic phase fault restores prior active release & preserves staged release.

`UpdateHooks::quiesce`, verification, & migration must be finite supervisor operations;
the engine samples cancellation before each phase. Migration implementers must make
`rollback_schema` undo both partial & complete schema changes; failures report all
schema/filesystem rollback errors while still attempting every restoration step.
Receipt publication is last & atomic. A pre-existing `.rollback` path is rejected;
post-success cleanup failure is returned so operators can repair it deterministically.
