import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';

const source = readFileSync(new URL('../../engine/crates/membrane-mcp/src/http_security.rs', import.meta.url), 'utf8');
const schema = JSON.parse(readFileSync(new URL('../../schemas/http-mcp-security.v1.schema.json', import.meta.url), 'utf8'));

test('HTTP admission is pure, loopback-only & keeps stdio default', () => {
  assert.match(source, /pub fn admit\(policy: &HttpAdmissionPolicy/);
  assert.match(source, /peer_ip\.is_loopback\(\)/);
  assert.match(source, /resolved_host_ip\.is_loopback\(\)/);
  assert.match(source, /HostNotAllowed|OriginNotAllowed|DnsRebinding/);
  assert.match(source, /MissingBearer|InvalidBearer|SessionMismatch|InstallationMismatch/);
  assert.match(source, /expected_bearer_token|expected_session_binding|constant_time_eq/);
  assert.doesNotMatch(source, /pub expected_session_binding: &'a str/);
  assert.match(source, /BodyTooLarge|DeadlineTooLong/);
  assert.doesNotMatch(source, /TcpListener|bind\(|hyper|axum|reqwest/);
  const lib = readFileSync(new URL('../../engine/crates/membrane-mcp/src/lib.rs', import.meta.url), 'utf8');
  assert.match(lib, /pub use jsonrpc::\{serve_stdio, McpServer\}/);
});

test('receipt schema is content-free & denial-typed', () => {
  assert.equal(schema.additionalProperties, false);
  assert.deepEqual(schema.required, ['accepted', 'transport']);
  assert.equal(schema.properties.transport.const, 'streamable_http');
  assert.equal(schema.properties.denial.enum.includes('dns_rebinding'), true);
  assert.equal(schema.properties.denial.enum.includes('invalid_bearer'), true);
  assert.equal('bearerToken' in schema.properties, false);
  assert.equal('sessionBinding' in schema.properties, false);
  assert.equal('body' in schema.properties, false);
});
