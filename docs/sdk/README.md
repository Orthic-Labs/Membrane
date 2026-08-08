# Membrane SDK contract

Python, TypeScript, & Rust clients expose only `membrane_context` plus
`membrane_source_read`. Each client takes an injected transport; none opens a
socket, retries, starts, stops, or discovers a daemon.

Every response must match canonical v1 envelopes in
`operations/operations/membrane-*.v1.golden.json`: `schemaVersion: 1`,
`errorVersion: 1`, matching operation, & a closed success or typed-error result.
Unknown fields, versions, operations, & error codes fail closed.

```js
import { MembraneClient } from "@orthic/membrane-client";

const client = new MembraneClient((operation, request) => transport(operation, request));
const context = client.context({ task: "inspect" });
```

`ProtocolError` exposes `code`, `message`, `retryable`, & `details`; values
match Python client behavior. Rust exposes matching constants, `ProtocolError`,
& boxed transport through `MembraneClient::new`.
