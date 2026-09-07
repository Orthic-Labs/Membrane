# Membrane branch consolidation audit

Date: 2026-09-07  
Authoritative remote inspected: `https://github.com/Orthic-Labs/Membrane.git`  
Main SHA: `a4e0a9e02c64e5439cdec289a160b748f5e5686c`

## Method

The review fetched every remote head into the read-only `refs/remotes/audit/*`
namespace without switching the checkout. Each non-main head was compared with
main by ancestry, patch identity (`git cherry`), changed paths, and the merged
history that superseded its work. The public GitHub pull-request API was also
queried by exact head name. No production file, qualification state, installed
state, or user data was changed.

An ancestry-only result is not sufficient here: most temporary branches were
created as patch-transfer or CI-staging branches and therefore have commits that
are not ancestors of main even though their intended product work was later
integrated under clean commits or sibling pull requests.

## Decision summary

No branch contains product work that should be merged into current main. All
nine non-main remote branches are safe to delete. One open pull request, #22,
uses one of those obsolete heads and should be closed rather than merged.

| Remote branch | Unique shape relative to main | Relevant landed replacement | Decision |
| --- | --- | --- | --- |
| `cortex-current-source` | One source-export workflow commit | Cortex lifecycle landed through PR #20 and later Cortex completion merges | Delete; temporary export transport |
| `cortex-governed-lifecycle` | 23 old implementation/staging/CI commits | PR #20 plus later PRs #23/#24 and the final Cortex completion merge | Delete; superseded implementation line |
| `cortex-governed-lifecycle-pr` | One clean-rebase export workflow commit | PR #20 | Delete; temporary export transport |
| `cortex-old-export` | The governed-lifecycle history plus one explicit old-tree export | PR #20 plus later Cortex completion work | Delete; explicitly historical export |
| `ledger-docs-stage` | Three patch-carrier/allowlist staging commits | Ledger end-to-end PR #15 and later resolver qualification PR #29 | Delete; staging transport, not product source |
| `ledger-finish-stage` | Two compiler-fix patch-carrier commits | Ledger end-to-end PR #15 and later main | Delete; staging transport, not product source |
| `pull-end-to-end-final-20260905` | One isolated final-validation workflow | Pull end-to-end PR #16 | Delete; one-shot validation lane |
| `work/pull-end-to-end-final-20260905` | One isolated validation workflow plus a patch part | Pull end-to-end PR #16 | Delete; patch/CI transport |
| `refactor-1/lane-e-coderight-binding-contract-current` | Six product commits are patch-equivalent to main; two generated CI refresh commits differ from newer main | CodeRight binding PR #21 | Close PR #22 and delete; obsolete duplicate head |

## Branch-by-branch evidence

### Cortex export and governed-lifecycle heads

The three export-labelled tips add only temporary GitHub workflows or an old
tree export. The larger `cortex-governed-lifecycle` line is based hundreds of
commits behind main and includes patch staging files, test-upload material, and
older generated/canon output. Main records the governed lifecycle merge as PR
#20 (`fb6c695c...`), then records the remaining Cortex work through PR #23,
PR #24, and final merge `a4e0a9e...`. Copying the older branch tip onto main
would regress newer source and generated truth rather than recover missing work.

### Ledger staging heads

Both Ledger heads modify only patch-transfer machinery under `scripts/ci/`.
Their merge bases predate the Ledger end-to-end merge by hundreds of current-main
commits. Main records the actual Ledger product merge as PR #15 (`34a3c0ab...`)
and the later resolver qualification as PR #29 (`f8493e0b...`). The carrier
branches have no remaining product delta to integrate.

### Pull final-validation heads

These heads contain only isolated CI workflow files and, in the `work/` head, a
split patch carrier. Main already records Pull end-to-end integration as PR #16
(`5e95595f...`). The old one-shot workflows are not current product behavior and
should not be merged.

### CodeRight binding duplicate head

`git cherry` shows the six functional commits on this head as patch-equivalent
to main. Main records their integrated form in PR #21 (`3548a6e...`). The two
remaining non-equivalent commits refresh generated migration manifests from an
older graph and are superseded by current main. GitHub reports PR #22 from this
exact head still open; merging it would duplicate landed work and risk stale
generated output.

## Remote cleanup status

Deletion was attempted only after the comparisons above. The environment has no
GitHub authentication (`gh auth status` reports no logged-in host), and an exact
non-interactive delete dry run failed before mutation with “could not read
Username”. Consequently, no remote branch or pull request was changed.

Once write authentication is available, the intended remote operation is to
close PR #22 without merging, delete the nine heads listed above, and re-enumerate
remote heads to prove that only `main` remains.
