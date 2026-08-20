# Blueprint container image

The `ghcr.io/orthic-labs/blueprint` image is for **CI and headless use only** —
not a desktop distribution. It runs the CLI under Node 22 LTS with update
checks disabled.

```sh
docker build -t ghcr.io/orthic-labs/blueprint:0.2.0 .
docker run --rm -v "$PWD:/repo" -w /repo ghcr.io/orthic-labs/blueprint status --json
```

The image does not run the resident watcher or the local explorer.
