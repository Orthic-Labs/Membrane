# Blueprint container image

The `ghcr.io/membrane/blueprint` image is for **CI and headless use only** —
not a desktop distribution. It runs the CLI under Node 22 LTS with update
checks disabled.

```sh
docker build -t ghcr.io/membrane/blueprint:0.2.0 .
docker run --rm -v "$PWD:/repo" -w /repo ghcr.io/membrane/blueprint status --json
```

The image does not run the resident watcher or the local explorer.
