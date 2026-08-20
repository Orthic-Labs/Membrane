# Update signing keys

RightKit `right-release` owns private updater keys, local signing, and rotation.
Never generate, inspect, print, store, or pass private signing material through
Blueprint source, commands, logs, manifests, or GitHub Actions.

Blueprint carries public trust roots only in
`lib/update/trusted-update-keys.json`. A public update is publishable only when
its sealed manifest is signed by a provisioned RightRelease key accepted by that
root. Missing or mismatched trust fails closed.

Build and seal first, then upload the exact release ID through the explicit
`update` lane:

```sh
pnpm exec right-release upload --release <release-id> --platform mac --tier update
```

Key rotation remains a RightRelease operation: add the new public key, ship one
overlap window accepting both current and previous keys, then retire the old
public key after compatibility verification.
