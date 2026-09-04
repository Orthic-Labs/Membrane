#!/usr/bin/env node
// Offline operator helper. Private keys remain on the operator machine; no
// enrollment, HTTP request, memory mutation or key material logging occurs.
import { createPrivateKey, sign } from 'node:crypto';
import { readFileSync } from 'node:fs';
import { pathToFileURL } from 'node:url';

const FIELDS = ['schemaVersion','policyVersion','installationId','cortexStoreId','repositoryId','scopeId',
  'operation','targetId','expectedContentHash','expectedControlRevision','keyId','nonce','issuedAtMs','expiresAtMs','signatureHex'];
const DOMAIN = Buffer.from('Membrane Cortex reviewed effect v1\0');

export function signingBytes(effect) {
  if (!effect || typeof effect !== 'object' || Array.isArray(effect)) throw new Error('review must be an object');
  for (const name of Object.keys(effect)) if (!FIELDS.includes(name)) throw new Error(`unknown field: ${name}`);
  const ordered = {};
  for (const name of FIELDS) {
    if (name === 'signatureHex') ordered[name] = '';
    else if (name === 'expectedControlRevision') ordered[name] = effect[name] ?? null;
    else {
      if (effect[name] === undefined) throw new Error(`missing field: ${name}`);
      ordered[name] = effect[name];
    }
  }
  if (ordered.schemaVersion !== 1 || ordered.policyVersion !== 'cortex-reviewed-effect-v1') throw new Error('unsupported review version');
  if (!['approve','reject','retry','suppress','resume'].includes(ordered.operation)) throw new Error('unsupported effect');
  for (const name of ['issuedAtMs','expiresAtMs']) if (!Number.isSafeInteger(ordered[name]) || ordered[name] <= 0) throw new Error(`invalid ${name}`);
  if (ordered.expiresAtMs <= ordered.issuedAtMs || ordered.expiresAtMs-ordered.issuedAtMs > 86400000) throw new Error('invalid review lifetime');
  if (!/^sha256:[0-9a-f]{64}$/.test(ordered.expectedContentHash)) throw new Error('expectedContentHash must name exact content');
  if (['retry','suppress','resume'].includes(ordered.operation) && typeof ordered.expectedControlRevision !== 'string') throw new Error('effect requires current decision revision');
  for (const name of ['installationId','cortexStoreId','repositoryId','scopeId','targetId','keyId','nonce']) {
    if (typeof ordered[name] !== 'string' || !ordered[name].trim()) throw new Error(`invalid ${name}`);
  }
  const bytes = Buffer.concat([DOMAIN, Buffer.from(JSON.stringify(ordered))]);
  if (bytes.length > 65536) throw new Error('review exceeds maximum size');
  return bytes;
}
export function signReviewedEffect(effect, privateKeyPem) {
  const key = createPrivateKey(privateKeyPem);
  if (key.asymmetricKeyType !== 'ed25519') throw new Error('review key must be Ed25519');
  return { ...effect, expectedControlRevision:effect.expectedControlRevision ?? null,
    signatureHex:sign(null, signingBytes(effect), key).toString('hex') };
}
if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  try {
    const [keyPath, effectPath, ...extra] = process.argv.slice(2);
    if (!keyPath || !effectPath || extra.length) throw new Error('usage: sign-reviewed-effect.mjs PRIVATE_KEY.pem EFFECT.json');
    const input = readFileSync(effectPath);
    if (input.length > 65536) throw new Error('effect exceeds maximum size');
    process.stdout.write(`${JSON.stringify(signReviewedEffect(JSON.parse(input),readFileSync(keyPath)),null,2)}\n`);
  } catch (error) { process.stderr.write(`review signing failed: ${error.message}\n`); process.exitCode=1; }
}
