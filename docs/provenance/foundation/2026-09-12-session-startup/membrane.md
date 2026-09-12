# SessionStart startup-context comparison

This source-bound comparison records one new Membrane capability introduced by
the 2026-09-12 startup-context request. It compares the current native hook
path against the frozen canon revision and keeps provider omissions, stale
Blueprint admission, and bounded host projection in scope.

| Atom | Scope | Competitive disposition | Best mechanism | Current evidence | Donor evidence | Gap / action |
|---|---|---|---|---|---|---|
| MEM-067 | COMMITTED | CURRENT_INCOMPLETE | Shared Membrane planner invoked by native SessionStart, with resident reuse and sealed-store one-shot fallback; bounded host `additionalContext` projection | Native SessionStart registration and dispatch are present in the current worktree; focused qualification remains pending | Donors expose lifecycle startup hooks or generated orientation files, but none provides this Membrane-owned cross-provider packet | Add installed-host qualification for Codex and Claude, then promote implementation/verification/qualification independently |
