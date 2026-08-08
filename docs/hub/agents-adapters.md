# Agent adapters

Hub exposes `agent-adapters.v1` as a bounded, read-only projection of externally assembled client/device evidence. Each adapter keeps separate `installed`, `active`, `delivering`, `enforced`, and `proven` states; each state carries mechanism and evidence.

`declaredCapability` is an assertion, not proof. When `clientCapability` is below it, any evidenced state is rendered `degraded` with mechanism `client capability below declared capability`. Missing evidence remains `unknown` with mechanism `unavailable`; the view never infers liveness from installation or declaration.
