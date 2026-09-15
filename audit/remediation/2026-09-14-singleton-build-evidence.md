# Membrane build & installed qualification evidence — 2026-09-14

Owner: current Codex task. User-authorized maximum: 3 installer/release builds. **Builds used: 3/3. Candidate3 built & installed; qualification FAILED.** Resumed hour07:04–08:04IST. Historical first-hour records below remain evidence for their own candidates.

Current installed candidate: generation `efb52b21fc331f38aa5770e29b860e00ac35c10ace979d8563a5c1763182961b`; installer SHA256 `0892d552653f28e949c106a4d8f4097b009545150447bece56fc21aa8a382c31`. Binding-only starts no engine; scheduled engine task absent. Tray-only hold, tray-led engine recovery & mixed-owner continuity passed. Final-owner process drain & repeated harness-led crash recovery failed. Functional Pull/Blueprint/Push/Cortex exact-byte readback/session hooks passed after manual test-state recovery. Concurrent Pull saturates MCP lane & blocks subsequent Push. Full installed qualifier failed Hub health & cleanup drain; upgrade/watcher/residue parity remains unqualified.

Post-build source fixes for bounded Tokio teardown & activation suppression reset have passing focused regressions, but are **not installed**. All source remains uncommitted. Raw final evidence: [singleton-final](2026-09-14-singleton-final/). No native-only seal or completion validation claimed.

## Before build 1 — NOT CLEARED

| Path traced | Failure risk | Required resolution/evidence |
|---|---|---|
| engine/crates/membrane/src/activation.rs::request_resident_replacement | Windows CommandExt missing at creation_flags | Luna added scoped import; managed compiler check pending |
| engine/crates/membrane/src/bin/membrane-client.rs::holder_exchange callers | Option<&Value> parameter passed bare &Value in Drop/Renew | Luna corrected both call sites; compiler check pending |
| client run/lease lifecycle | CLI acquires no owner; hook drops owner before direct HTTP startup | Bounded CLI ownership & session-aware hook lease fix in progress |
| activation.rs::activate / activate_bindings | Engine starts before first holder; bindings must never start it | Binding branch traced; bootstrap owner-loss bound & readiness still under review |
| membrane-runtime/src/serve.rs::resident_holder / expiry loop | Final release can race new owner; background/lifetime distinction | Production shutdown trace & focused residency tests pending |
| membrane-runtime/tests/singleton_architecture_qualification.rs | Tests use private model, not production engine | Do not accept model suite as installed or production lifecycle proof |
| apps/membrane-hub/src-tauri/windows/installer.nsi | Same-version overwrite locks; task resurrection; binding under install lock | Latest commits remove task, release install lock before binding; exact source recheck pending |
| scripts/qualification/install-release.ps1 | Health timeout/indexing ambiguity; installed mutation sequence | Latest typed degraded response & exact installed identity checks require validation |
| migration/native-rust runtime manifest + invocation graph | New code invalidates digests | Regenerate after source stabilizes, both checks must pass |
| scripts/release/local-windows-development.ps1 | Builds, qualifies then installs again | Use supported build route deliberately; inspect failure before retry, never blind rebuild |

## Acceptance still required

Production owner acquire/renew/release/expiry, simultaneous activation, Hub only/harness only/both/neither, bounded final drain, no scheduled resurrection; installed exact source identity; Blueprint parity, hooks/Pull/Push & 1/5/20-client idle/indexing measurements. Source tests & installed results recorded separately. No completion claim from module existence or compilation.

## Final source preflight

- Windows activation tests: 30 passed, RightKit 1229e0d7-cac4-430a-a997-64a69fee1b1e. Missing CommandExt resolved in real Windows compilation.
- Forwarding binary tests 12 passed & boundary tests 2 passed, RightKit 558bff2f-c076-4a73-9649-6e70db66928a; log `.tmp-singleton-client-tests-final.log`. Earlier invalid atomic-store ordering was corrected before this successful check.
- Runtime/service tests: 30 passed, RightKit b11f43ca-1a46-4a4a-96fc-f3f25860f646; log `.tmp-singleton-runtime-tests.log`. Test-only missing axum body import corrected before successful run.
- Node main/restored suites: 92 passed, 1 existing skipped; legal inventory passed. Log `.tmp-singleton-node-tests.log`.
- Installer structural contract: 14 passed after replacing obsolete direct-engine startup expectations. Existing installed qualification contract: 13 passed.
- Runtime manifest & invocation graph: errors=0, warnings=0, prodInterpreterRows=0. Runtime inventory & product docs checks passed. Regenerate once more after final source freeze.

### Traced corrections & failure containment

1. Client `run` requires a holder before forwarding. CLI/stdin sessions renew, hook session identity refreshes a bounded lease, SessionEnd releases. Renew/release use current controller identity; renewal recovery reacquires after generation change through installed activation. Hidden activation is bounded at 15 seconds; Drop release at 2 seconds. No effectful fully-sent request retries.
2. Client holder wire operation uses protocol snake_case. Signed lifecycle requests & livez reuse bounded socket pool; ordinary calls reset socket timeouts. No runtime/store dispatch added to client.
3. `serve::resident-holder` serializes controller updates & final drain. Hub/CodeRight counts authorize background work; harness count only holds engine. Expiry revokes background authority even when harness survives. Initial ownerless installed transport has a 30-second grace then drains.
4. Activation accepts healthy installed identity/catalog/store without requiring unauthorized background watching. Corrupt/unavailable component assertions remain fail-closed.
5. Installer disables/removes legacy task, stops product processes, overlays version tree with bounded retries, verifies executables, stages junction & restores prior junction on cutover failure. Binding runs `--bindings-only`; login migration changes exact legacy engine command to tray & preserves absent/foreign choices. Same-version image locks remain an installed scenario to exercise, not a proven success.
6. Supported unsigned route builds engine/client/Cortex together, then tray, Hub & NSIS. Native source checks precede this route. Qualification reuses resulting installer/manifest/SBOM; installer failure will be diagnosed against its step/file logs before any second build.

Installed qualification is intentionally outstanding: these checks clear source compilation & known path defects only. Build count remains 0/3 until release command starts.

## Preflight observations (04:15 IST)

- Managed client residency integration: 5 passed (RightKit da590ce8-03d4-480d-bf9d-6d4a5641184a).
- Managed client library/registry/auth: 27 passed (RightKit a636e117-12a5-4fe2-b81a-a3e96822fcdd).
- Installer qualification contract tests: 13 passed before targeted login migration; rerun after final edit.
- First residency test compile exposed missing harness_holders fixture; corrected before successful run. This was a focused test compile, not an installer/release build attempt.
- Additional production blockers discovered: holder wire enum incorrectly sent PascalCase; Renew/Release ignored original controller identity; harness ownership granted background authority; initial ownerless startup lacked expiry; activation required watcher readiness without background grant; upgrade marker preserved old engine login command. All require source fix/review before packaging.
- No runtime/install effects performed by this task yet. Existing installed engine/task reflect previous artifact; do not confuse old installation with source under qualification.
- Historical singleton_architecture_qualification tests model lifecycle privately; cannot substitute for production registry/route tests or installed matrix.

## Build 1 clearance

Focused checks above passed. Final installer checks: 27 passed. Cleanup checks refreshed after source freeze: both zero errors. Extraction now logs exact failed target; stop probe checks every overwritten product executable. Parent corrected loop/pipeline ambiguity before build.

Base commit: 3dbb0d8bcfe6e077e4c3a4146fbf10d1a731157b. Candidate includes these uncommitted source hashes; release identity records dirty source-tree digest.

| Candidate file | SHA256 |
|---|---|
| apps/membrane-hub/src-tauri/windows/installer.nsi | da0a7d502a7e0d4fa58b22732f64330b62896fa1a84fa1b6ecbf2676fcde7566 |
| engine/crates/membrane-client/tests/residency.rs | 23028ef787a23059c3037cc34b4d7bc635e15f54f1cfe93cfc13db4c52c48cdf |
| engine/crates/membrane-runtime/src/serve.rs | b4be136bcaf71d5e9078428e5cd705896b40957607ae1930dc256acdd193fdbb |
| engine/crates/membrane-runtime/src/service.rs | e8764d8fcaa2c69a99f0a92ae0cd61d397c3bfc5f754409175430f2cc00e16db |
| engine/crates/membrane/src/activation.rs | 9488c8c64478accf6eb06a6818fdafea4ddc346f6733179e80aab929251b7066 |
| engine/crates/membrane/src/bin/membrane-client.rs | d6965209766d62af532c4388710027c93a20b283d7d7e6f141d5a5f3a7976785 |
| migration/native-rust/invocation-graph.json | 1d08ad5ff5de5ce7b456ee8449a517dbe1a147235ac3db56a249ddcb4d567812 |
| migration/native-rust/runtime-language-manifest.json | 5d66f322d1ec94ca23c6c9c77ce75ae218f4a5cacd83a770a0f974fa2c675fa7 |
| scripts/qualification/install-release.test.mjs | deae2763427adbee02be9cc21c04448b7e97ce392bb15aff7c1552330bcc233f |
| scripts/qualification/installer-nsi-activation-contract.test.mjs | 829e62b13f84e1d1cf8e44a27ebc166932b0c37a2b648cb7e7866f17c44184b8 |

Build 1 started 2026-09-14T04:29:24.7960537+05:30. Attempts used: 1/3. Command: pnpm --dir apps/membrane-hub run release:build:win:unsigned. Log: .tmp-singleton-build1.log.
Candidate engine generation emitted by build 1: sha256:9580ff2663654e27b16170f3f5efccebf8ea3a6e0521b7ba19401509cc547910 (dirty source, 1006 files). Installed identity must match this exact value.

Post-clearance finding during qualification preparation: modes.rs::dispatch_cli generic tail still calls runtime CLI locally. Existing installed forwarding path is /cli via membrane-client, but legacy engine CLI must be redirected before final singleton closure. No claim of full architecture qualification for candidate1. A separate patch is being prepared outside frozen source; installer/lifetime qualification of candidate1 still provides actual evidence before another build.
Build 1 command exited 0 at 2026-09-14T04:43:24.7251178+05:30.
Build1 verified installer SHA256 71fddf7a18dc5ca8f6248723d50eb69ea37157a1aaec70003c81e338a23ce212. Installing exact candidate at 2026-09-14T04:44:18.4154933+05:30.
Build1 installer exit=0.

## Candidate 1 installed result
Installer exit0; SHA25671fddf7a18dc5ca8f6248723d50eb69ea37157a1aaec70003c81e338a23ce212. Installed build-info generation matches9580ff2663654e27b16170f3f5efccebf8ea3a6e0521b7ba19401509cc547910. Legacy scheduled task absent. Runtime observed one resident owner plus temporary hook activation controls. Holder acquisition does not complete: signed_post_engine duplicates complete SDK Host/Content-Length/Content-Type/Connection headers, including contradictory close & host authority. This invalid wire boundary is not exercised by prior source grep tests. Build2 must fix canonical framing & test actual SDK header output, plus legacy CLI generic forwarding. Candidate1 is not lifetime-qualified; do not claim binding-only zero-engine because live harness hooks immediately resumed access.

## Build 2 preflight clearance

Candidate1 failed holder acquisition because signed requests duplicated canonical headers. SDK verifier also requires Connection: close; preserve SDK headers exactly once. Ordinary request pooling remains unchanged. Independent installed wire probe now returns HTTP200 for acquire & release; evidence .tmp-holder-wire-proof.json. New canonical-header regression passes. Managed client13 + boundary2 tests pass (.tmp-singleton-build2-client.log); modes9 pass (.tmp-singleton-build2-focused.log). Both refreshed cleanup checks report zero errors/warnings.

Traced paths: signed_post_engine -> render_signed_request -> SDK canonical headers -> server verifier -> resident holder registry; no override of authenticated framing. Legacy CLI generic dispatch -> installed canonical sibling path validation -> membrane-client cli -> resident HTTP dispatch; development sibling rejected, health/build-info retain stateless local metadata paths. No payload hashing per CLI call. Child inherits user streams with Windows hidden launch.

Failure review: wire rejection reproduced & corrected against actual installed server. Full installed client requires new payload. Installer uses same bounded extraction/cutover/binding path proven by candidate1 successful install. Concurrent external harness hooks can reacquire runtime, so zero-owner assertions require controlled observation; do not infer ownerlessness from closed Hub. Crash renewal & actual operations remain installed acceptance gates. No new release machinery or retry introduced.

Build2 authorized for these identified corrections. Build budget becomes2/3 only on command start. Hard stop remains05:08 IST; qualification must report actual completed gates only.
apps/membrane-hub/src-tauri/windows/installer.nsi SHA256=DA0A7D502A7E0D4FA58B22732F64330B62896FA1A84FA1B6ECBF2676FCDE7566
engine/crates/membrane-client/tests/residency.rs SHA256=23028EF787A23059C3037CC34B4D7BC635E15F54F1CFE93CFC13DB4C52C48CDF
engine/crates/membrane-runtime/src/serve.rs SHA256=B4BE136BCAF71D5E9078428E5CD705896B40957607AE1930DC256ACDD193FDBB
engine/crates/membrane-runtime/src/service.rs SHA256=E8764D8FCAA2C69A99F0A92AE0CD61D397C3BFC5F754409175430F2CC00E16DB
engine/crates/membrane/src/activation.rs SHA256=9488C8C64478ACCF6EB06A6818FDAFEA4DDC346F6733179E80AAB929251B7066
engine/crates/membrane/src/bin/membrane-client.rs SHA256=AB1699264EB89D2D45935B6198BCC4D1EDA9AF7148409B0B6D3CD52468B1BB44
engine/crates/membrane/src/modes.rs SHA256=1558A178C3FAA43627718BBE848C49F0E21614D5DEC5199530A65153D6A5D778
migration/native-rust/invocation-graph.json SHA256=CE2CAFB82E00396991BB8D8E24359BBAA977A2E69441AEFB2557CD5E340A7DA0
migration/native-rust/runtime-language-manifest.json SHA256=0FEC8B0A527C5EB2D8B40F5744378E0B464702E0B7ECBA9368F037593ECFFA97
scripts/qualification/install-release.test.mjs SHA256=DEAE2763427ADBEE02BE9CC21C04448B7E97CE392BB15AFF7C1552330BCC233F
scripts/qualification/installer-nsi-activation-contract.test.mjs SHA256=829E62B13F84E1D1CF8E44A27EBC166932B0C37A2B648CB7E7866F17C44184B8
Build2 started 2026-09-14T04:53:16.2081346+05:30; attempts2/3; log .tmp-singleton-build2.log.
Build2 exit=0 at 2026-09-14T05:06:48.6503790+05:30.
Build2 installer SHA256=0ea42c05ae1610a35d777bb15bcfd5908ce1e64e135659302a1726a987be6bbc verified; install start 2026-09-14T05:06:58.5684362+05:30.
Build2 installer exit=2 at 2026-09-14T05:07:44.7569257+05:30.


## Hard stop — 05:08 IST

Stopped at user deadline. Both release builds compiled successfully; attempts2/3, no third build. Candidate2 generation4938825a3b0b0c07f552cef412eafacd4bf3b3adb6330c3ea83a30799ad446c5, installer SHA2560ea42c05ae1610a35d777bb15bcfd5908ce1e64e135659302a1726a987be6bbc. Installer exit2: extraction, verify, junction stage, cutover & register succeeded; bind-installed-clients exit1. Installed identity readback, client smoke & lifetime helper did not execute because installer failure stopped command. Do not call installation successful or qualification complete.

Source preserved uncommitted. Focused24 tests & both cleanup checks passed for build2. Wire probe against candidate1 acquired/released actual harness holder with HTTP200. Full lifetime/Blueprint/hook/Pull/Push/performance qualification remains unexecuted. Full qualification runner also has stale Invoke-NativeMcp $native.Membrane call at scripts/qualification/install-release.ps1:1418; requires transport client before running. No source modification after candidate2 freeze.

Next concrete investigation on explicit resume: read binding failure payload, determine why binding-only control returned1 after verified cutover, then reuse this exact artifact if repair does not change packaged code. No blind rebuild.

## Resumed hour — 07:04–08:04 IST; build 3 preflight

User authorized one further hour, not additional builds. Builds used remain 2/3 before this command. Hard stop 08:04 IST. Source remains on main at 3dbb0d8bcfe6e077e4c3a4146fbf10d1a731157b with owned uncommitted changes.

Candidate2 installation failure was diagnosed from installed logs/bindings.log: activation startup lock remained busy. Installer killed its recorded owner; old recovery required both dead PID & 90-second age, while acquisition waited only15seconds. Same candidate binding-only succeeds after age expires & starts no engine. Full qualifier reproduced the same failure on fresh installation; no blind rebuild.

Final source paths: installer stop/cutover -> installed membrane activate --bindings-only -> activation StartupLock -> permanent OS-locked .activation.guard -> legacy directory ownership read/reclaim/publication. Guard spans whole acquisition lifetime & serializes concurrent reclaim. Known exited process is immediately recoverable; unknown/unpublished owner retains stale grace; live process remains excluded. Windows GetExitCodeProcess handles an exited but retained process handle. Two race/dead-owner tests plus remaining activation tests pass31/31 (.tmp-resume-activation-final.log).

Second diagnosed defect: installed tray --login-launch produced controllerActive=false, hubHolders=0 (.tmp-resume-tray-holder-status.json). Dashboard had been sole Hub holder. Final path main.rs retained InstalledHubLease -> installed_holder.rs worker -> signed fenced resident Acquire/Renew/Release -> engine holder registry. Tray holds Hub lease independent of dashboard; worker reacquires after startup generation change & requests bounded hidden installed activation after endpoint failure. Shutdown wakes worker, joins & releases original fenced lease. Crash loses lease through30second TTL. Main supervisor detaches without killing surviving harness runtime. No task/autostart is introduced. startup.rs activation is hidden with15000ms deadline. Full managed tray suite43/43, including stopped-worker & immediate-drop tests (.tmp-resume-tray-tests-final.log).

Candidate2 actual installed evidence: five simultaneous stdio clients share one runtime; closing four preserves survivor; forced engine failure recovers; final client loss stops engine (.tmp-singleton-installed-lifetime.json). Transport tools/list1/5/20 measured separately; not claimed as Pull performance. Actual Pull succeeds with honest provider omissions; Blueprint status succeeds; native Push stores exact UTF8/trailing-space bytes & Cortex readback matches; SessionStart/SessionEnd hooks pass (.tmp-resume-functional.json). These observations apply to candidate2 until repeated on final artifact.

Qualification harness paths corrected: installed native init enrolls unique fixture root in canonical registry before Hub startup; no prohibited fixture override against canonical endpoint. Cleanup removes only unique run binding under registry lock. MCP transport uses installed membrane-client; public tool listing is pull/push; engine identity matches installed path rather than tray parent PID. Qualification runner13tests pass (.tmp-resume-qual-tests-final.log). Full upgrade/watcher/residue qualification remains an installed gate, not source-test success.

Failure review before final build: compile/link tested on actual Windows managed toolchain; no packaged source changes pending. Activation lock guards live owners against concurrent reclaim, young dead owner immediately recovers. Tray release cannot terminate surviving harness; fenced identity prevents stale generation mutation. Startup activation bounded & hidden. Installer hash/generation must match candidate manifest before install. Final installed verification must prove tray-only ownership, mixed-owner continuity, final drain & candidate identity. Full qualifier's native enrollment cleanup may encounter external registry lock; fail visibly rather than overwrite unrelated binding. No unresolved source failure is being bypassed. Both regenerated accounting checks report0errors/0warnings; native-only seal NOT issued.
engine/crates/membrane/src/activation.rs SHA256=D11EF2DE329887D00CBDD0009F6471C4C22994B7596F37DB9FB2F3FC964D65C3
apps/membrane-tray-windows/src/installed_holder.rs SHA256=E52BBC9F4080293BFEFCE8A153C0187AA2ED02D0EA206203ED7B62C2D632C976
apps/membrane-tray-windows/src/main.rs SHA256=2ABC85DB21A406885F96894C4AB2905FFADF8A5487B4590A3A0AA7DAA94222C9
apps/membrane-tray-windows/src/startup.rs SHA256=6D7F5AE6ABC66F5A566931E3C968EF9E7A249A25993ED79E6BB751C5DCE4170A
apps/membrane-tray-windows/src/supervisor.rs SHA256=ED0F3FED232AE2B3945770D14AA87003064B8CE3DE41598F5EEF726F1EA0902C
scripts/qualification/install-release.ps1 SHA256=1DB95A3CA7F61D039CF0699D32D1F228A16B07A68A9073ED019D5D88ADC1B353
migration/native-rust/runtime-language-manifest.json SHA256=86C6D0D37F8F1D1E3C77010C5B1322F1134904FFFBA55648B262A6C28FB42B3E
migration/native-rust/invocation-graph.json SHA256=DF489A43222D71AE4D9DF5FB583F43AF36AA17FCC3D8205F3F5FC81F82CF1078
Build3 started 2026-09-14T07:30:21.8070061+05:30; attempts3/3; log .tmp-singleton-build3.log.
Course checkpoint07:34IST: six-step scope retained; build3/3 running after focused31+43 & cleanup checks passed. Same heartbeat updated to hard stop08:04IST. No additional release build authorized. Engine release identity covers engine subtree; tray source hashes above & final installer hash bind tray separately.
Final pnpm test passed92, skipped1, failed0; legal source inventory passes (.tmp-final-pnpm-test.log). Engine release phase155342ms & tray release phase178378ms completed; Hub/package still running.
Build3 exit=0 at 2026-09-14T07:44:03.2600938+05:30.
Build3 installer SHA256=0892d552653f28e949c106a4d8f4097b009545150447bece56fc21aa8a382c31 verified; install start 2026-09-14T07:44:24.0856620+05:30.
Build3 installer exit=0 at 2026-09-14T07:44:55.5498243+05:30.
Final silent install & explicit binding-only leave zero engine processes; obsolete task absent.
Build3 installed client health exit=0.
Build3 installed legacy CLI Blueprint status exit=1.

## Candidate3 installed evidence — not complete

Silent installer exit0 at07:44:55IST. Installer SHA2560892d552653f28e949c106a4d8f4097b009545150447bece56fc21aa8a382c31; installed generationefb52b21fc331f38aa5770e29b860e00ac35c10ace979d8563a5c1763182961b matches. Scheduled task absent. Silent install & explicit binding-only leave zero engines. Installed client health passes. Immediate following legacy CLI Blueprint status fails harness ownership acquisition (.tmp-singleton-build3-blueprint-status.log); do not claim forwarding qualification passed.

Actual tray lifetime evidence .tmp-final-tray-lifetime.json: initial tray-only holder acquired & same oneengine survives35s; enginePID228240 deliberately crashed, tray recovered newengine206320/newgeneration384 & Hub holder; harness joined; tray crash losesHub holder whileharness preserves sameengine. FINAL DRAIN FAILED: afterclient closes, listener unavailable but engine206320 remains >40seconds. Primary stopped only this exact test-owned installedengine after recording failure. Candidate3 is NOT lifetime-qualified. Luna assigned narrow teardown diagnosis/fix, no additional build authorized; any later source fix must be labeled uninstalled.

Further candidate3 gates: five simultaneousharnesses shareoneengine, but forcedcrash recovery timesout30seconds (.tmp-final-harness-lifetime.json). Fresh initialize thenfails20seconds. Direct installed activation reports15secondhealthtimeout. Actual engine-stderr.log identifies restart suppression after3uncleanexits; activation no longer resets this legacy state. This is a product defect, not a qualification PASS. Supported installed deactivate invoked to clear failurestate for isolated remaining functional checks; it also removes installed bindings, which must be restored by binding-only/install beforefinish. Candidate3 automaticrecovery remainsFAILED regardless manual reset.

Source teardown inspection also finds temporary Tokio runtime implicitDrop after5secondHTTPdrain, which can indefinitely wait on spawn_blocking tasks. Source repair now retains Runtime & bounds its shutdown; focused regression pending. These post-build source repairs are NOT in installedcandidate3 & are NOT qualification evidence for it.

After explicit diagnostic bare installed launch (no scheduled task) & supported deactivation marked supervision clean, deactivation itself waited on its own mapped executable in first_locked_file; primary stopped only its identified test-owned controlPID230676, then restored binding-only registrations. This is an additional deactivation defect, not a supported automatic recovery PASS. No supervision JSON hand-edit performed.

Candidate3 functional checks after manual recovery PASS: Pull success, Blueprint status success, Push success with exactUTF8/trailing-space Cortex readback, SessionStart/SessionEnd ok. Evidence audit/remediation/2026-09-14-singleton-final/functional-after-manual-recovery.json carries exactlivezidentity. Initial fresh harness initialization failure before recovery remains recorded separately in direct-activation-failure.log & engine-stderr.log.

Candidate3 established HTTP performance: singlePull5770ms success; fiveconcurrentPull2success/1typederror/2RPCfailures; twentyPullallHTTP503 deadline-exceeded around15020ms. SubsequentPush1/5/20 allHTTP429 mcp_lane_saturated. These are failed saturation qualification results, not successfulthroughput. Idle-vs-indexing comparison, CPU/memory/queue-delay/connectionreuse baseline remainunqualified. Rawrequests/timing in singleton-final/performance.json.

Post-build source repairs: authorized activation resets stale supervision failurestate before launch (no scheduled task); Tokio runtime explicitly shuts down within5seconds after HTTPdrain. Controlledblockingtask regression PASSED; suppressionresetregression compiling. Both refreshed cleanupchecks0errors/0warnings. These changes are NOT installed; buildbudget3/3 exhausted. Full installed qualifier against exactcandidate3 launched07:57:22IST; installer/activation passed, Hub verification underway.

## Resumed-hour final state

Both post-build regressions pass: controlled blockedspawn_blocking shutdown returns withinbound; previouslysuppressed state resets clean=true/failures=0 beforeauthorizedlaunch. Manifest & invocationgraph refreshedagain;0errors/0warnings. Sourcefixes notinstalled, no4thbuild.

Fullcandidate3 installedqualifier failedHubhealth afterinstaller/activation/nativehostcutover/dashboard stages; watcher unavailable, backgroundauthorityactive, HTTP503. Its one-time watcherextension did not produce expected signaturelog; cause stillunresolved. Cleanup thenfailedfinal-holderdrain. Upgrade/watcher/package-residue/Blueprintseal gates notcompleted.

Primary stopped ONLY qualification-owned tray143988 & engine189644 afterbounded installeddeactivation request. Supervisionstateclean=true/failures=0, binding-only restoredsuccessfully, noinstalledproductprocessremained atreadback. Noengineautostarttaskcreated. Source & evidenceuncommitted, no push. No fullRustworkspace suite, native-onlyseal or independentcompletionvalidation claimed.

Next implementation targets fromactualevidence: qualify currentuninstalledteardown/resetfixes on anew explicitlyauthorizedcandidate; correct deactivation's ownimage lockwait; diagnose why cancelled/expired Pull retainsMCPcapacity & rejectsPush; repairqualifierhealthsignature/reporting &finishinstalledwatcher/upgrade/residueparity. Do not callinstalledarchitecturecomplete untilthoseacceptancegates pass.
Hard stop completed at 2026-09-14T08:04:24.0252433+05:30. Heartbeat paused; workers completed; no new work scheduled. No installed product processes at final readback. Builds3/3. Source & receipts preserved uncommitted.
