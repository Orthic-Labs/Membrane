// WinGet multi-file manifest (version / installer / defaultLocale) for the
// Windows channel this repository actually ships to: an NSIS installer
// signed under packaging/windows/release-contract.json (MBR-902) and bound
// to Winget/Scoop by packaging/winget/release.v1.json (MBR-905 source
// contract, verified by ../verify-windows-channels.mjs). This module only
// ever reads that already-verified contract; it never invents a version,
// URL, or hash of its own.
//
// WinGet's real, installable format is three YAML files per version, laid
// out the way the community winget-pkgs repo requires:
//   manifests/o/Orthic/Membrane/<version>/Orthic.Membrane.yaml            (version)
//   manifests/o/Orthic/Membrane/<version>/Orthic.Membrane.installer.yaml  (installer)
//   manifests/o/Orthic/Membrane/<version>/Orthic.Membrane.locale.en-US.yaml (defaultLocale)

export const PACKAGE_IDENTIFIER = "Orthic.Membrane";
export const MANIFEST_VERSION = "1.6.0";
export const DEFAULT_LOCALE = "en-US";

const VERSION_RE = /^[0-9]+\.[0-9]+\.[0-9]+$/;
const SHA256_RE = /^[0-9a-f]{64}$/;

function httpsUrl(value) {
  try {
    const parsed = new URL(value);
    return parsed.protocol === "https:" && !parsed.username && !parsed.password;
  } catch {
    return false;
  }
}

/**
 * Builds the three WinGet manifests from a windows-channels contract
 * (packaging/winget/release.v1.json shape) whose `state` is already
 * "ready" -- callers must run it through
 * scripts/release/verify-windows-channels.mjs#verifyWindowsChannels first,
 * which proves sourceIdentity/artifact/channels.winget are cross-bound to a
 * passing MBR-902 receipt. This function copies those already-proven
 * fields; it does not re-derive or trust anything unverified.
 */
export function buildWingetManifests(contract) {
  if (contract?.state !== "ready") {
    throw new Error("windows channel contract is not ready: refusing to build an installable WinGet manifest");
  }
  const version = contract.sourceIdentity.version.replace(/^v/, "");
  const winget = contract.channels?.winget ?? {};

  const version_manifest = {
    PackageIdentifier: PACKAGE_IDENTIFIER,
    PackageVersion: version,
    DefaultLocale: DEFAULT_LOCALE,
    ManifestType: "version",
    ManifestVersion: MANIFEST_VERSION,
  };

  const installer_manifest = {
    PackageIdentifier: PACKAGE_IDENTIFIER,
    PackageVersion: version,
    MinimumOSVersion: "10.0.17763.0",
    InstallerType: "nsis",
    Scope: "user",
    InstallModes: ["silent", "silentWithProgress", "interactive"],
    UpgradeBehavior: "install",
    Installers: [
      {
        Architecture: "x64",
        InstallerUrl: winget.installerUrl,
        InstallerSha256: winget.installerSha256,
        InstallerSwitches: { Silent: "/S", SilentWithProgress: "/S" },
      },
    ],
    ManifestType: "installer",
    ManifestVersion: MANIFEST_VERSION,
  };

  const locale_manifest = {
    PackageIdentifier: PACKAGE_IDENTIFIER,
    PackageVersion: version,
    PackageLocale: DEFAULT_LOCALE,
    Publisher: "Orthic",
    PackageName: "Membrane",
    License: "UNLICENSED",
    ShortDescription: "Membrane assembles minimal current context packets and receipts from typed local providers.",
    ManifestType: "defaultLocale",
    ManifestVersion: MANIFEST_VERSION,
  };

  return { version: version_manifest, installer: installer_manifest, locale: locale_manifest };
}

/**
 * The intentionally-broken trio committed while the channel is
 * "unavailable" (no signed Windows release exists yet). Every field WinGet
 * needs to actually install something -- PackageVersion, InstallerUrl,
 * InstallerSha256 -- carries a declared sentinel that validateWingetManifests
 * rejects. This mirrors packaging/homebrew/Casks/membrane.rb's `raise`: a
 * fail-closed placeholder, never a plausible-looking fake version/URL/hash.
 */
export function unavailableWingetManifests() {
  const UNAVAILABLE_URL = "UNAVAILABLE: generate only from packaging/winget/release.v1.json once state is ready";
  const UNAVAILABLE_SHA256 = "UNAVAILABLE-generate-from-release-v1-json-once-state-is-ready";
  const PackageVersion = "0.0.0-unavailable";
  return {
    version: {
      PackageIdentifier: PACKAGE_IDENTIFIER,
      PackageVersion,
      DefaultLocale: DEFAULT_LOCALE,
      ManifestType: "version",
      ManifestVersion: MANIFEST_VERSION,
    },
    installer: {
      PackageIdentifier: PACKAGE_IDENTIFIER,
      PackageVersion,
      MinimumOSVersion: "10.0.17763.0",
      InstallerType: "nsis",
      Scope: "user",
      InstallModes: ["silent", "silentWithProgress", "interactive"],
      UpgradeBehavior: "install",
      Installers: [
        {
          Architecture: "x64",
          InstallerUrl: UNAVAILABLE_URL,
          InstallerSha256: UNAVAILABLE_SHA256,
          InstallerSwitches: { Silent: "/S", SilentWithProgress: "/S" },
        },
      ],
      ManifestType: "installer",
      ManifestVersion: MANIFEST_VERSION,
    },
    locale: {
      PackageIdentifier: PACKAGE_IDENTIFIER,
      PackageVersion,
      PackageLocale: DEFAULT_LOCALE,
      Publisher: "Orthic",
      PackageName: "Membrane",
      License: "UNLICENSED",
      ShortDescription: "Membrane Windows release is intentionally unavailable until signed release evidence exists.",
      ManifestType: "defaultLocale",
      ManifestVersion: MANIFEST_VERSION,
    },
  };
}

/**
 * Fails closed on any manifest trio that is not genuinely installable:
 * inconsistent identifiers/versions across the three files, a non-SemVer
 * PackageVersion (rejects the "0.0.0-unavailable" placeholder), a non-https
 * InstallerUrl (rejects the UNAVAILABLE sentinel), a non-hex-64
 * InstallerSha256 (rejects the UNAVAILABLE sentinel), missing silent
 * switches, or a missing locale field. This is the same authority a real
 * winget-pkgs manifest validator enforces for the fields that matter to
 * install/upgrade/uninstall/version-detection.
 */
export function validateWingetManifests({ version, installer, locale } = {}) {
  for (const [name, manifest] of [["version", version], ["installer", installer], ["locale", locale]]) {
    if (manifest?.PackageIdentifier !== PACKAGE_IDENTIFIER) throw new Error(`${name} manifest: PackageIdentifier must be ${PACKAGE_IDENTIFIER}`);
    if (manifest?.ManifestVersion !== MANIFEST_VERSION) throw new Error(`${name} manifest: ManifestVersion mismatch`);
  }
  if (!VERSION_RE.test(version.PackageVersion)) throw new Error("version manifest: PackageVersion must be strict SemVer, not a placeholder");
  if (version.ManifestType !== "version" || version.DefaultLocale !== DEFAULT_LOCALE) throw new Error("version manifest malformed");

  if (installer.PackageVersion !== version.PackageVersion) throw new Error("installer manifest: PackageVersion must match version manifest");
  if (installer.ManifestType !== "installer" || installer.InstallerType !== "nsis") throw new Error("installer manifest malformed");
  if (installer.UpgradeBehavior !== "install") throw new Error("installer manifest: UpgradeBehavior must enable installed-version upgrade detection");
  if (!Array.isArray(installer.Installers) || installer.Installers.length !== 1) throw new Error("installer manifest: exactly one Installers entry required");
  const [entry] = installer.Installers;
  if (entry.Architecture !== "x64") throw new Error("installer manifest: Architecture must be x64");
  if (!httpsUrl(entry.InstallerUrl)) throw new Error("installer manifest: InstallerUrl must be a real https URL, not a placeholder");
  if (!SHA256_RE.test(entry.InstallerSha256)) throw new Error("installer manifest: InstallerSha256 must be a real SHA-256, not a placeholder");
  if (entry.InstallerSwitches?.Silent !== "/S" || entry.InstallerSwitches?.SilentWithProgress !== "/S") throw new Error("installer manifest: silent install switches must be /S");

  if (locale.PackageVersion !== version.PackageVersion) throw new Error("locale manifest: PackageVersion must match version manifest");
  if (locale.ManifestType !== "defaultLocale" || locale.PackageLocale !== DEFAULT_LOCALE) throw new Error("locale manifest malformed");
  if (!locale.Publisher || !locale.PackageName || !locale.License || !locale.ShortDescription) throw new Error("locale manifest: Publisher/PackageName/License/ShortDescription required");

  return { version, installer, locale };
}

function isPlainObject(value) {
  return value !== null && typeof value === "object" && !Array.isArray(value);
}

function scalar(value) {
  if (typeof value === "string") return JSON.stringify(value);
  if (typeof value === "boolean" || typeof value === "number") return String(value);
  throw new Error(`renderWingetYaml: unsupported scalar type ${typeof value}`);
}

function renderBlock(node, depth) {
  const pad = "  ".repeat(depth);
  let out = "";
  for (const [key, value] of Object.entries(node)) {
    if (Array.isArray(value)) {
      out += `${pad}${key}:\n`;
      for (const item of value) {
        if (isPlainObject(item)) {
          const itemPad = "  ".repeat(depth + 1);
          const lines = renderBlock(item, depth + 1).split("\n").filter((line) => line.length > 0);
          out += `${itemPad}- ${lines[0].trim()}\n`;
          for (const line of lines.slice(1)) out += `${"  ".repeat(depth + 1)}${line}\n`;
        } else {
          out += `${"  ".repeat(depth + 1)}- ${scalar(item)}\n`;
        }
      }
    } else if (isPlainObject(value)) {
      out += `${pad}${key}:\n${renderBlock(value, depth + 1)}`;
    } else {
      out += `${pad}${key}: ${scalar(value)}\n`;
    }
  }
  return out;
}

/**
 * Minimal deterministic YAML serializer for exactly the shapes these three
 * manifests use (flat scalars, one nested object, one single-entry array of
 * objects, one array of strings). No YAML package is installed in this
 * workspace and installing one is out of scope for this task, so this is a
 * hand-rolled, narrowly-scoped emitter rather than a general YAML library.
 * Every string scalar is JSON-quoted (a valid YAML scalar form), which
 * avoids hand-written escaping logic entirely.
 */
export function renderWingetYaml(node) {
  return renderBlock(node, 0);
}
