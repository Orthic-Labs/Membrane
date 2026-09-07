import test from 'node:test';
import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { mkdtemp, mkdir, realpath, rm, writeFile, readFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import { createHash } from 'node:crypto';

const root = resolve(new URL('../../..', import.meta.url).pathname);
const server = join(root, 'mcp/server.mjs');
const fixture = join(root, 'audit/session-b/fixtures/repo-alpha');
const sha256 = (bytes) => createHash('sha256').update(bytes).digest('hex');

async function rpc(messages, env) {
  const child = spawn(process.execPath, [server], {stdio:['pipe','pipe','pipe'], windowsHide:true, env:{...process.env,...env}});
  let stdout='', stderr='';
  child.stdout.on('data', c => { stdout += c; }); child.stderr.on('data', c => { stderr += c; });
  child.stdin.end(messages.map(JSON.stringify).join('\n')+'\n');
  const code = await Promise.race([
    new Promise((res, rej) => { child.once('error',rej); child.once('close',res); }),
    new Promise((_,rej) => setTimeout(() => { child.kill(); rej(new Error('bounded RPC timeout')); }, 7000)),
  ]);
  assert.equal(code, 0, stderr);
  return stdout.trim().split('\n').filter(Boolean).map(JSON.parse).filter(x => x.id !== undefined);
}

async function isolatedBinding() {
  const data = await mkdtemp(join(tmpdir(),'membrane-dogfood-b-'));
  const repo = await realpath(fixture);
  const registry = join(data,'registry.json');
  await writeFile(registry, JSON.stringify({schema_version:1,bindings:{[repo]:{repository_id:'dogfood-alpha',scope_id:'dogfood-scope-alpha',provider_config:{},grant_policy:{level:'read'}}}}));
  return {data,repo,registry};
}

function validateCapturedEvidence(capture, expected) {
  assert.equal(capture.repositoryId, expected.repositoryId, 'repository identity mismatch');
  assert.equal(capture.sourceDigest, expected.sourceDigest, 'stale/wrong digest accepted');
  for (const fact of expected.requiredFacts) assert.ok(capture.body.includes(fact), `missing required fact: ${fact}`);
  assert.equal(new Set(capture.bodies).size, capture.bodies.length, 'duplicate evidence body accepted');
  assert.ok(capture.omissions.includes('ledger_unavailable'), 'required omission was dropped');
  assert.notEqual(capture.diagnostics?.status, 'clean', 'fake clean diagnostics accepted');
}

test('ORACLE-NEG controls reject independently mutated captures', async () => {
  const bytes = await readFile(join(fixture,'src/reconnect.ts'));
  const expected = {repositoryId:'dogfood-alpha', sourceDigest:sha256(bytes), requiredFacts:['WAKE_RETRY_LIMIT = 3','never retry after explicit logout']};
  const good = {repositoryId:'dogfood-alpha', sourceDigest:expected.sourceDigest, body:bytes.toString(), bodies:['one','two'], omissions:['ledger_unavailable'], diagnostics:{status:'unavailable'}};
  assert.doesNotThrow(() => validateCapturedEvidence(good, expected));
  const mutations = [
    {...good, repositoryId:'dogfood-beta'}, {...good, sourceDigest:'0'.repeat(64)},
    {...good, body:'irrelevant'}, {...good, bodies:['same','same']},
    {...good, omissions:[]}, {...good, diagnostics:{status:'clean'}},
  ];
  for (const mutation of mutations) assert.throws(() => validateCapturedEvidence(mutation, expected));
});

test('A01 retained public MCP discovery advertises a callable context schema', async () => {
  const iso=await isolatedBinding();
  try {
    const rows=await rpc([
      {jsonrpc:'2.0',id:1,method:'initialize',params:{protocolVersion:'2025-03-26',capabilities:{},clientInfo:{name:'dogfood-session-b',version:'1'}}},
      {jsonrpc:'2.0',id:2,method:'tools/list',params:{}},
    ],{MEMBRANE_PROJECT_REGISTRY:iso.registry,MEMBRANE_PORT:'65531',MEMBRANE_API_TOKEN:'isolated-invalid-token'});
    const context=rows[1].result.tools.find(t=>t.name==='membrane_context');
    assert.ok(context,'context not discovered');
    assert.deepEqual(context.inputSchema.required,['task','repository','caller']);
    assert.ok(context.outputSchema,'missing output schema');
  } finally { await rm(iso.data,{recursive:true,force:true}); }
});

test('B01 retained public MCP Hub-off response is canonical typed unavailable', async () => {
  const iso=await isolatedBinding();
  try {
    const rows=await rpc([{jsonrpc:'2.0',id:3,method:'tools/call',params:{name:'membrane_context',arguments:{task:'find reconnect loop after wake',repository:iso.repo,caller:{root:iso.repo,repositoryId:'dogfood-alpha',scopeId:'dogfood-scope-alpha'}}}}],{MEMBRANE_PROJECT_REGISTRY:iso.registry,MEMBRANE_PORT:'65531',MEMBRANE_API_TOKEN:'isolated-invalid-token'});
    const payload=JSON.parse(rows[0].result.content[0].text);
    assert.deepEqual(payload,{error:{code:'membrane_unavailable',reason:'hub_inactive',retryable:true}});
  } finally { await rm(iso.data,{recursive:true,force:true}); }
});
