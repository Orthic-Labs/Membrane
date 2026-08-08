# Python client

`membrane-client` is source-ready only; no PyPI publication claim exists.

```python
from membrane_client import MembraneClient

client = MembraneClient(transport)  # transport(operation, request) -> response envelope
packet = client.context({"task": "inspect"})
```

Client owns no daemon discovery, HTTP client, socket, or retry loop. Callers inject
loopback transport. It accepts only published operation envelope v1, validates exact
operation/error versions, closed envelopes, plus closed error codes, & raises
`ProtocolError` on every malformed or unknown response. `analyze_packet` summarizes
ContextPacket v1 blocks/omissions. `analyze_receipt` summarizes current per-candidate
ContextReceipt v2 & prior v1 admission/status fields; other versions fail closed.

Real daemon compatibility & PyPI distribution remain acceptance gates.
