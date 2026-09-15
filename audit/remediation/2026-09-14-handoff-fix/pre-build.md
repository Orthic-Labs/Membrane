# Handoff fix pre-build evidence

Authorized: fix capture/handoff, one installer build, install & test. Existing dirty changes preserved; no commit/push.

Root cause evidence: prior activation-pipe-owner.json proves successful activation exit precedes stderr closure until daemon dies. New client spawn_activation captures stderr in unique file, waits on process handle only; regression passed with descendant retaining stderr. No output() pipe wait remains on activation path.

Handoff flow: initial signed acquire → start activate --engine-only → poll signed acquire concurrently in short exchanges → return lease immediately when acquired → bounded child reaper. Single startup deadline bounds retry loop; transport exchanges capped at one second. Lease signature & identity checks unchanged. Engine-only activation bypasses client registration/PATH/credential/hook reconciliation; normal installer activation unchanged. Engine remains singleton under existing lock. No per-minute task introduced.

Focused verification: client suite 16 passed; control library 95 passed; final parser suite 17 passed including explicit engine-only & conflicting binding-only rejection. Diff whitespace check passed. No source edits after freeze. Canonical unsigned pipeline compiles release feature binaries, stages engine/client/tray & packages Hub/NSIS. Verify source drift, packaged generation, file hashes & installed identity before tests.

Remaining risks: existing Blueprint shutdown & Pull timing omission are outside this startup fix. Startup must be measured with original 20-second initialize window; no expanded timeout to hide failure. Lifecycle regression includes surviving owner, engine crash recovery & final owner drain. Installer extraction locking is diagnosed from install log rather than rebuilt blindly. Background reaper deadline termination affects only its own engine-only activation control child, not daemon.
