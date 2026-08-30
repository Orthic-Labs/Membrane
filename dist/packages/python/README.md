# membrane-client

Thin, transport-injected Python client for Membrane protocol envelopes, plus
content-free analysis helpers (`analyze_packet`, `analyze_receipt`). It never
opens a socket, discovers a daemon, retries, or bundles the Membrane daemon,
its binaries, or any other package's source: callers inject the transport
function that reaches their own local Membrane installation.

```python
from membrane_client import MembraneClient

client = MembraneClient(transport)  # transport(operation, request) -> response envelope
packet = client.context({"task": "inspect"})
```

Full contract, compatibility-test pointers, and the distribution-boundary
policy that keeps this package a client SDK and never the core application:
see `docs/reference/sdk/python.md` and `docs/reference/sdk/python-publishing.md` in the Membrane
repository root.
