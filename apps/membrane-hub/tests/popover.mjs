import test from 'node:test'; import assert from 'node:assert/strict';
import { viewModel, diagnostics } from '../src/popover.mjs';
test('deterministic severity and explicit statuses',()=>{ const vm=viewModel({payload:{overall:{state:'available'},sources:{state:'degraded'},reason:'one'},observed_at_unix_ms:1}); assert.equal(vm.overall,'Degraded'); assert.equal(vm.sources,'Degraded'); assert.equal(vm.reason,'one'); });
test('trace is unavailable without explicit id and diagnostics contain no payload',()=>{ const vm=viewModel({payload:{fleet:{state:'unavailable'},secret:'nope'}}); assert.equal(vm.traceId,null); assert.doesNotMatch(diagnostics(vm),/nope/); });
