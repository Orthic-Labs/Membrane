# Delivery trace explorer

Hub exposes a read-only `delivery-trace.v1` projection for one trace, ordered as task → providers → candidates → admission → render → host delivery → evidence → outcome → feedback.

`packet`, `host`, `event-store`, and `outcome` receipts each contribute an opaque digest. The projection reports `available` only when all four are present and byte-identical; missing receipts are `unavailable`, while any mismatch is `degraded`. Hub never infers successful delivery from process state, phase labels, or a partial trace.
