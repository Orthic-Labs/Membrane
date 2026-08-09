# Update signing keys

Cortex accepts portable/native updates only when an Ed25519 signature matches
its shipped trust root.

## Generate offline

Run on an offline machine & put private material outside this repository:

```sh
node scripts/release/generate-update-keys.mjs --out /secure/path/cortex-update-signing.pem
```

Generator requires `--out`, refuses repository-local output, writes PKCS8 PEM
with mode `0600` where supported, & prints only public PEM plus `keyId`.
`keyId` is first 16 lowercase hex characters of SHA-256 over SPKI DER.

Store private PEM in a password manager or hardware-backed vault. Create GitHub
Actions repository secret `UPDATE_SIGNING_KEY_PEM` with exact PEM contents;
never add private PEM to source, logs, manifests, or workflow files.

## Add public key

Add printed values to `lib/update/trusted-update-keys.json` using loader fields:

```json
{
  "schemaVersion": 1,
  "keys": [{
    "keyId": "<printed keyId>",
    "algorithm": "Ed25519",
    "publicKey": "-----BEGIN PUBLIC KEY-----\n...\n-----END PUBLIC KEY-----"
  }]
}
```

## Sign

```sh
UPDATE_SIGNING_KEY_PEM="$(cat /secure/path/cortex-update-signing.pem)" \
  node scripts/release/sign-update-manifest.mjs --manifest /path/to/manifest.json
```

Signer uses `canonicalManifestPayload` from `lib/update/manifest.mjs` & writes
`signature`, derived `keyId`, & `signatureAlgorithm: "Ed25519"` in place.

## Rotate

Append new public key, sign next release with it, retain previous N-1 key for
one release window, then remove retired key.

## Fail closed

Shipped root is `{"schemaVersion":1,"keys":[]}`. Empty root rejects every real
update by design until owner completes ceremony & adds first public key.
