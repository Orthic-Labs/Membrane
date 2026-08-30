export function sidecarBuildCommand({ environment = process.env, platform = process.platform } = {}) {
  if (environment.MEMBRANE_PUBLIC_CI_DIRECT_CARGO === "1") return { command: "cargo", prefix: [] };
  return { command: environment.RIGHTKIT || (platform === "win32" ? "rightkit.cmd" : "rightkit"), prefix: ["cargo"] };
}
