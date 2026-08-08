# Team policy synchronization

`TeamPolicySyncV1` carries only tenant, team, user, generation, policy digest, offboarding, key-rotation, audit-export, & encrypted-envelope identifiers. It carries no content, keys, storage location, or transport instruction.

Scopes are explicit: tenant, team, user, & local root. Wire policy may cover only tenant/team/user; runtime rejects local-root scope. Trusted verification must confirm user-origin learning scope preservation, so team policy cannot broaden either boundary.

`TeamPolicyTrustVerifier` supplies encryption & authorization results from trusted crypto/policy integration. Callers cannot assert either result in wire data. Admission bounds IDs, scopes, nonces, & offboarding before calling trusted verification, then rejects non-increasing generations, missing encryption, or missing authority. Receipt binds policy, tenant/team, generation, envelope ID, ciphertext digest, decision, & stable snake-case reason.

Offboarding is represented by user IDs, key rotation by an opaque rotation ID, & audit export by an opaque export ID. Real crypto, key lifecycle, transport, persistence, and export remain integration gates.
