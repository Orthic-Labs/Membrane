# Quickstart fixture

`run.mjs` is a deterministic, offline rehearsal of MBR-1002. It checks a service marker, validates an enrollment receipt, emits one packet, then demonstrates forced degradation. It does not replace live MCP acceptance.

```sh
node run.mjs
node run.mjs --degraded
```
