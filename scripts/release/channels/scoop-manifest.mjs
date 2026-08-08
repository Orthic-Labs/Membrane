// Scoop bucket manifest for the same signed NSIS installer the WinGet
// manifest (./winget-manifest.mjs) binds, taken from the identical
// windows-channels contract (packaging/winget/release.v1.json, verified by
// ../verify-windows-channels.mjs#verifyWindowsChannels before either builder
// runs). Scoop's real, installable format is already a single JSON file
// (packaging/scoop/membrane.json), unlike WinGet's three-file YAML manifest,
// so this module only adds the fields Scoop needs to actually
// install/upgrade/uninstall an NSIS-based app: `installer`/`uninstaller`
// silent-flag stanzas and `checkver`/`autoupdate` version detection.

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
 * Builds the ready-state Scoop manifest from a "ready" windows-channels
 * contract. Every URL/hash/version comes from the already-verified
 * contract; nothing here invents an artifact identity.
 */
export function buildScoopManifest(contract) {
  if (contract?.state !== "ready") {
    throw new Error("windows channel contract is not ready: refusing to build an installable Scoop manifest");
  }
  const version = contract.sourceIdentity.version.replace(/^v/, "");
  const scoop = contract.channels?.scoop ?? {};
  const installerName = contract.artifact?.name;

  return {
    version,
    description: "Membrane assembles minimal current context packets and receipts from typed local providers.",
    homepage: "https://github.com/orthic/membrane",
    license: "UNLICENSED",
    url: scoop.url,
    hash: `sha256:${scoop.sha256}`,
    architecture: {
      "64bit": {
        installer: { file: installerName, args: ["/S"] },
        uninstaller: { file: "uninstall.exe", args: ["/S"] },
      },
    },
    checkver: { url: "https://github.com/orthic/membrane/releases/latest", regex: "Membrane_([\\d.]+)_x64-setup\\.exe" },
    autoupdate: {
      architecture: {
        "64bit": { url: "https://github.com/orthic/membrane/releases/download/v$version/Membrane_$version_x64-setup.exe" },
      },
      hash: { url: "$url.sha256" },
    },
  };
}

/**
 * The committed unavailable-state manifest (packaging/scoop/membrane.json):
 * no `url`/`hash` key at all -- Scoop cannot resolve an artifact for this
 * app -- plus an explicit `notes` field declaring why. Exported so tests and
 * the generator can assert the on-disk file matches this exact fail-closed
 * shape instead of duplicating the literal object in multiple places.
 */
export function unavailableScoopManifest() {
  return {
    version: "0.0.0-unavailable",
    description: "Membrane is intentionally unavailable until signed Windows release evidence exists.",
    homepage: "https://github.com/orthic/membrane",
    license: "UNLICENSED",
    notes: "Do not install: this source contract has no artifact URL or checksum.",
  };
}

/**
 * Fails closed on any manifest that is not genuinely installable: missing
 * version, a `url` without a matching `sha256:`-prefixed 64-hex `hash`, a
 * missing/blank silent installer or uninstaller flag, or a checkver/
 * autoupdate block that cannot detect a new version. An unavailable-shaped
 * manifest (no `url` key) is accepted as long as it also carries no `hash`,
 * `installer`, or `uninstaller` key -- i.e. it cannot silently half-install.
 */
export function validateScoopManifest(manifest) {
  if (!manifest || typeof manifest !== "object") throw new Error("scoop manifest: must be an object");
  if (typeof manifest.version !== "string" || manifest.version.length === 0) throw new Error("scoop manifest: version required");

  const hasUrl = "url" in manifest;
  const hasInstaller = "architecture" in manifest || "installer" in manifest;
  if (!hasUrl && !hasInstaller) {
    if ("hash" in manifest) throw new Error("scoop manifest: unavailable manifest must not declare a hash without a url");
    return manifest;
  }

  if (!httpsUrl(manifest.url)) throw new Error("scoop manifest: url must be a real https URL, not a placeholder");
  const hashValue = manifest.hash;
  if (typeof hashValue !== "string" || !hashValue.startsWith("sha256:") || !SHA256_RE.test(hashValue.slice("sha256:".length))) {
    throw new Error("scoop manifest: hash must be sha256:<64-hex>, not a placeholder");
  }

  const arch = manifest.architecture?.["64bit"];
  if (!arch) throw new Error("scoop manifest: architecture.64bit stanza required");
  if (!arch.installer?.file || !Array.isArray(arch.installer.args) || !arch.installer.args.includes("/S")) {
    throw new Error("scoop manifest: architecture.64bit.installer must run with silent flag /S");
  }
  if (!arch.uninstaller?.file || !Array.isArray(arch.uninstaller.args) || !arch.uninstaller.args.includes("/S")) {
    throw new Error("scoop manifest: architecture.64bit.uninstaller must run with silent flag /S");
  }

  if (!httpsUrl(manifest.checkver?.url) || typeof manifest.checkver?.regex !== "string" || manifest.checkver.regex.length === 0) {
    throw new Error("scoop manifest: checkver must carry a real https url and a non-empty regex for version detection");
  }
  const autoArch = manifest.autoupdate?.architecture?.["64bit"];
  if (typeof autoArch?.url !== "string" || !autoArch.url.includes("$version")) {
    throw new Error("scoop manifest: autoupdate.architecture.64bit.url must be templated with $version");
  }
  if (typeof manifest.autoupdate?.hash?.url !== "string" || manifest.autoupdate.hash.url.length === 0) {
    throw new Error("scoop manifest: autoupdate.hash.url required for version-detected upgrade");
  }

  return manifest;
}
