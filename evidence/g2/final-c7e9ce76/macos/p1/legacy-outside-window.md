# Mac P1 pre-window accounting

Current accepted observation window is installation `be8b2353-c0f5-4250-867c-22c5629bd4e8`,
service instance `ceda637d-c0cd-4e3c-bfbb-f6320e6aea58`, from
`2026-07-27T19:21:44Z` through `2026-07-27T19:21:46.837336Z`.

The immutable prior capture
`reconciliation-bounded.json` (SHA-256
`8f904a97794860e3804cdb1e219e54e22123d7cb28b9fb61267864e643182451`) records 225
`delivery_missing_value_terminal` gaps through its own cutoff
`2026-07-27T18:03:42.149258Z`. It is retained unchanged.

Bounded indexed accounting before this accepted instance additionally observes six later delivered
blocks on its service instance plus 9 on generation 131 & 8 on generation 132, all outside current
window. No event was synthesized, altered, or deleted. The accepted generation-133 receipt has
8 events & zero gaps.
