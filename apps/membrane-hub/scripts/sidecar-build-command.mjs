export function sidecarBuildCommand({ environment = process.env, platform = process.platform } = {}) {
  if (environment.MEMBRANE_PUBLIC_CI_DIRECT_CARGO === "1") {
    if (environment.GITHUB_ACTIONS !== "true") throw new Error("direct Cargo is reserved for GitHub Actions; local installers require RightKit");
    return { command: "cargo", prefix: [] };
  }
  return { command: environment.RIGHTKIT || (platform === "win32" ? "rightkit.cmd" : "rightkit"), prefix: ["cargo"] };
}
