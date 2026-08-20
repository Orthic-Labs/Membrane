# Membrane macOS workspace installation package

`workspace/` is canonical source for workspace bootstrap logic. Generate its
release projection with:

```sh
node membrane/scripts/generate-install-workspace.mjs
node membrane/scripts/generate-install-workspace.mjs --check
```

Run package-context tests on macOS without placing runtime tests in shipped package:

```sh
PYTHONDONTWRITEBYTECODE=1 python3 -m pytest membrane/install/tests
PYTHONDONTWRITEBYTECODE=1 MEMBRANE_WORKSPACE_TEST_ROOT=membrane/dist/install/workspace python3 -m pytest membrane/install/tests
```

Generated `dist/install/workspace/` is validated by
`dist/install/workspace-manifest.json`; setup accepts it only when every
declared file, byte count, SHA-256 digest, schema, & runtime requirement match.
No compatibility module is provided for retired service or product names.
