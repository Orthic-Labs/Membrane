// MBR-912: machine-checkable, fail-closed release-channel compatibility
// policy -- the JavaScript mirror of
// engine/crates/membrane-protocol/src/compatibility_policy.rs. This is the
// enforcement point Hub/CLI JS code is meant to call before ever reporting
// a channel/support/required-update decision as unambiguous; the schema
// (`release-channel.v1.schema.json`) and README describe the shape and
// intent in prose, this module is the code that actually decides.
//
// Refuses by default on exactly the three cases MBR-912 names, plus the
// same "schemaCompatibility must be proven, not assumed" rule the Rust
// policy enforces:
//   1. an unknown channel (anything other than stable/beta/nightly),
//   2. an unsupported release-channel schema version (anything other than
//      RELEASE_CHANNEL_SCHEMA_VERSION), and
//   3. a downgrade (candidate `release` not a strictly greater
//      major.minor.patch than the locally installed version) -- using the
//      identical version-triple parsing rule membrane-updater's Rust
//      `is_upgrade` and this crate's `compatibility_policy::is_strict_upgrade`
//      both use, so all three copies agree.
//   4. an explicitly "unknown" or "incompatible" schemaCompatibility value.
//
// evaluateCompatibility() takes a raw, untyped descriptor -- exactly what
// would arrive over IPC or a CLI flag before any schema validation -- so
// "unknown channel" is a real runtime-checked branch, not an input the type
// system already makes unreachable. evaluateReleaseChannel() adapts an
// already schema-valid descriptor object through the same function, so
// there is exactly one enforcement path.
//
// Depends on nothing outside Node's standard library (no ajv, no build
// step): this module is dependency-free by design so it can run today via
// `node --test tests/compat/release-channel-compatibility.test.mjs`.

export const RELEASE_CHANNEL_SCHEMA_VERSION = 1;

const KNOWN_CHANNELS = new Set(["stable", "beta", "nightly"]);
const KNOWN_SCHEMA_COMPATIBILITY = new Set([
  "compatible",
  "migration_required",
  "incompatible",
  "unknown",
]);

export class CompatibilityViolation {
  constructor(code, detail) {
    this.code = code;
    this.detail = detail;
  }
}

const violation = (code, detail) => new CompatibilityViolation(code, detail);

function parseChannel(raw) {
  if (KNOWN_CHANNELS.has(raw)) return { ok: true, value: raw };
  return { ok: false, violation: violation("release_channel_unknown", { channel: raw }) };
}

function parseSchemaCompatibility(raw) {
  if (KNOWN_SCHEMA_COMPATIBILITY.has(raw)) return { ok: true, value: raw };
  return {
    ok: false,
    violation: violation("release_channel_schema_compatibility_unknown", { schemaCompatibility: raw }),
  };
}

// Parses the leading major.minor.patch numeric triple, tolerating a
// pre-release/build suffix on the patch component (e.g. "2.1.0-rc.1") --
// identical semantics to membrane-updater's Rust `version_triple` and this
// repo's `compatibility_policy::version_triple`.
function versionTriple(version) {
  const parts = String(version ?? "").split(".");
  if (parts.length < 3) return null;
  const leadingDigits = (part) => {
    const match = /^\d+/.exec(part);
    return match ? Number(match[0]) : null;
  };
  const major = leadingDigits(parts[0]);
  const minor = leadingDigits(parts[1]);
  const patch = leadingDigits(parts[2]);
  if (major === null || minor === null || patch === null) return null;
  return [major, minor, patch];
}

function compareTriples(a, b) {
  for (let i = 0; i < 3; i += 1) {
    if (a[i] !== b[i]) return a[i] < b[i] ? -1 : 1;
  }
  return 0;
}

// Same strictly-greater-triple upgrade rule as the Rust side: { ok: true,
// isUpgrade } when both versions parse; { ok: false, offending } naming
// whichever version string failed to parse when either does not -- fail
// closed either way, but reported so the caller knows which failure it saw.
function isStrictUpgrade(current, candidate) {
  const from = versionTriple(current);
  const to = versionTriple(candidate);
  if (from === null) return { ok: false, offending: current };
  if (to === null) return { ok: false, offending: candidate };
  return { ok: true, isUpgrade: compareTriples(to, from) > 0 };
}

/**
 * The one fail-closed enforcement path. `candidate` is a raw, untyped
 * descriptor: { schemaVersion, channel, release, schemaCompatibility }.
 * Every check runs; every violation is collected, never short-circuited on
 * the first failure. Returns { admitted: true, channel, release,
 * migrationRequired } only when the channel is known, the schema version is
 * current, schema compatibility is explicitly "compatible" or
 * "migration_required", and `release` is a strictly greater version than
 * `currentVersion`. Otherwise returns { admitted: false, violations }.
 */
export function evaluateCompatibility(currentVersion, candidate) {
  const violations = [];

  const channelResult = parseChannel(candidate?.channel);
  if (!channelResult.ok) violations.push(channelResult.violation);

  const schemaVersion = candidate?.schemaVersion;
  if (schemaVersion !== RELEASE_CHANNEL_SCHEMA_VERSION) {
    violations.push(violation("release_channel_schema_version_unsupported", { schemaVersion }));
  }

  const compatibilityResult = parseSchemaCompatibility(candidate?.schemaCompatibility);
  let migrationRequired = false;
  if (!compatibilityResult.ok) {
    violations.push(compatibilityResult.violation);
  } else if (compatibilityResult.value === "incompatible") {
    violations.push(violation("release_channel_schema_incompatible", {}));
  } else if (compatibilityResult.value === "unknown") {
    violations.push(
      violation("release_channel_schema_compatibility_unknown", { schemaCompatibility: "unknown" }),
    );
  } else {
    migrationRequired = compatibilityResult.value === "migration_required";
  }

  const upgrade = isStrictUpgrade(currentVersion, candidate?.release);
  if (!upgrade.ok) {
    violations.push(violation("release_channel_version_unparseable", { version: upgrade.offending }));
  } else if (!upgrade.isUpgrade) {
    violations.push(
      violation("release_channel_downgrade_rejected", {
        current: currentVersion,
        candidate: candidate?.release,
      }),
    );
  }

  if (channelResult.ok && violations.length === 0) {
    return {
      admitted: true,
      channel: channelResult.value,
      release: candidate.release,
      migrationRequired,
    };
  }
  return { admitted: false, violations };
}

/**
 * Convenience entry point for a caller that already holds a schema-valid
 * release-channel object (matching release-channel.v1.schema.json's
 * camelCase field names). Runs through the identical evaluateCompatibility
 * path -- no separate "trusted" fast path exists.
 */
export function evaluateReleaseChannel(currentVersion, schemaValidDescriptor) {
  return evaluateCompatibility(currentVersion, {
    schemaVersion: schemaValidDescriptor?.schemaVersion,
    channel: schemaValidDescriptor?.channel,
    release: schemaValidDescriptor?.release,
    schemaCompatibility: schemaValidDescriptor?.schemaCompatibility,
  });
}
