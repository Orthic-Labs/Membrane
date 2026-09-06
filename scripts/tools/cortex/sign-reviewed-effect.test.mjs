import test from 'node:test';
import assert from 'node:assert/strict';
import {generateKeyPairSync,verify} from 'node:crypto';
import {signingBytes,signReviewedEffect} from './sign-reviewed-effect.mjs';
const effect=()=>({schemaVersion:1,policyVersion:'cortex-reviewed-effect-v1',installationId:'install',cortexStoreId:'store',repositoryId:'repo',scopeId:'scope',operation:'approve',targetId:'proposal',expectedContentHash:`sha256:${'1'.repeat(64)}`,keyId:'reviewer',nonce:'nonce',issuedAtMs:1,expiresAtMs:60001});
test('signed effects verify only for their exact scope and payload',()=>{
 const {privateKey,publicKey}=generateKeyPairSync('ed25519');
 const signed=signReviewedEffect(effect(),privateKey.export({type:'pkcs8',format:'pem'}));
 assert.equal(verify(null,signingBytes(signed),publicKey,Buffer.from(signed.signatureHex,'hex')),true);
 assert.equal(verify(null,signingBytes({...signed,scopeId:'another'}),publicKey,Buffer.from(signed.signatureHex,'hex')),false);
});
test('wire order cannot change signed bytes and unsupported assertions fail closed',()=>{
 const e=effect();assert.deepEqual(signingBytes(e),signingBytes(Object.fromEntries(Object.entries(e).reverse())));
 assert.throws(()=>signingBytes({...e,reviewer:'admin'}));
 assert.throws(()=>signingBytes({...e,operation:'suppress'}));
 assert.throws(()=>signingBytes({...e,expiresAtMs:86400002}));
});
