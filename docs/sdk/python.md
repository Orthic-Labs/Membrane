# Python client

`membrane-client` (`packages/python/`) is packaging-ready, not published: its
distribution boundary, metadata, and version (`0.1.0`) are in place and
tested, but no PyPI publish has happened and none is triggered by this
source change. See `docs/sdk/python-publishing.md` for the publishing
policy and the "publish the SDK, not the core app" distribution boundary.

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

## Daemon compatibility

`schemas/context-receipt.v1.schema.json` types `ContextReceiptV1.schemaVersion`
as `1 or 2`: `2` is what a current daemon emits, `1` is what a previous
supported daemon emitted and the client must still accept.
`tests/sdk/python_daemon_compatibility_test.py` reads that enum from the
schema at test time (not a hard-coded copy) and proves `analyze_receipt`
accepts every version the schema currently lists as supported, keeps
correct non-degraded semantics for the oldest of them, and fails closed
(`ProtocolError` with `protocol_version_unsupported`) against a version
newer than any the schema lists. This is the "passes compatibility tests
against current and previous supported daemon" acceptance criterion this
task must demonstrate.

`tests/sdk/python_package_boundary_test.py` is the companion distribution
test: it proves the package that would carry these guarantees onto PyPI
ships nothing but this client. Real live-daemon round-trip compatibility
(as opposed to schema/fixture-level compatibility) remains blocked on the
`tools/call` stub documented in `docs/sdk/http-transport.md`; see
`docs/sdk/python-publishing.md`'s "Known gap".
