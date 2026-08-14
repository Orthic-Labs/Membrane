# Orthic Rules

## Purpose
- Orthic is product-neutral suite shell, lifecycle supervisor, onboarding surface, & installer owner.
- Keep Cortex graph truth & Membrane context/memory truth outside Hub.

## Canonical sources
- Read `README.md` for repository behavior.
- Read `solimplement.md` for current implementation sequence & status.
- Read workspace `docs/plans/orthic/SEAM-CONTRACT.md` for cross-product ownership.

## Commands
- Run focused JavaScript checks through pinned pnpm scripts.
- Run Rust checks through workspace RightKit after reading `docs/rules/rightkit.md`.
- Run native packaging only through RightKit release lane on native host.

## Locked invariants
- Keep manifest, lifecycle, & snapshot schemas product-neutral, versioned, bounded, & content-addressed.
- Keep secrets/live endpoints out of static manifests.
- Consume signed product artifacts by exact digest; never build from product source.
- Keep Hub state separate from Cortex & Membrane stores/key bytes.
- Supervise every child through authenticated fencing, drain, update handoff, & full-tree cleanup.

## Verification
- Test invalid version/range/digest/mode/path/symlink/secret inputs.
- Test only-Cortex & both-product tab sets.
- Prove quit, off, crash, update, rollback, & uninstall leave zero orphan children on Mac & Windows.

Before sealing any contract touching hub, watcher lifecycle, the cortex↔membrane API, or peer-service discovery, read `docs/plans/orthic/SEAM-CONTRACT.md` and declare it a dependency.
