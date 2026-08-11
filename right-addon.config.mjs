export default {
  schema: 1,
  kind: "headless-addon",
  addon: "membrane",
  version: "0.1.0",
  packageManager: "pnpm",
  checks: ["test", "test:all"],
  buildInputs: {
    include: [
      "right-addon.config.mjs", "package.json", "pnpm-lock.yaml",
      "engine/Cargo.toml", "engine/Cargo.lock", "engine/crates/**",
      "install/assets/membrane-tab-icon.png",
      "LICENSE", "EULA.txt", "PRIVACY.md", "THIRD-PARTY-NOTICES.txt",
    ],
    exclude: ["**/target/**", "**/tests/**"],
  },
  consumer: {
    contract: "orthic-product-v1",
    productId: "membrane",
    displayName: "Membrane",
    hubCompatRange: ">=0.1.11 <0.2.0",
    commandRole: "command",
    serviceRole: "service",
    iconRole: "icon",
    serviceStop: ["SIGTERM"],
    statusEndpoint: {
      host: "127.0.0.1",
      port: 47851,
      authHeader: "Authorization",
      tokenRelativePath: "tools/.cache/memory/api-token",
    },
  },
  targets: {
    mac: {
      targetTriple: "aarch64-apple-darwin",
      files: [
        { role: "command", name: "membrane", source: "engine/target/aarch64-apple-darwin/release/membrane", executable: true },
        { role: "service", name: "crypt-service", source: "engine/target/aarch64-apple-darwin/release/crypt-service", executable: true },
        { role: "icon", name: "membrane-tab-icon.png", source: "install/assets/membrane-tab-icon.png" },
        { role: "license", name: "LICENSE", source: "LICENSE" },
        { role: "eula", name: "EULA.txt", source: "EULA.txt" },
        { role: "privacy", name: "PRIVACY.md", source: "PRIVACY.md" },
        { role: "third-party-notices", name: "THIRD-PARTY-NOTICES.txt", source: "THIRD-PARTY-NOTICES.txt" },
      ],
      build: {
        cmd: "cargo",
        args: ["build", "--manifest-path", "engine/Cargo.toml", "--workspace", "--locked", "--release", "--target", "aarch64-apple-darwin", "-p", "membrane", "--bin", "membrane", "-p", "crypt", "--bin", "crypt-service"],
      },
      signing: { contract: "apple-developer-id-executable-v1", teamId: "6KLGD3LLKF" },
    },
    win: {
      targetTriple: "x86_64-pc-windows-msvc",
      files: [
        { role: "command", name: "membrane.exe", source: "engine/target/x86_64-pc-windows-msvc/release/membrane.exe", executable: true },
        { role: "service", name: "crypt-service.exe", source: "engine/target/x86_64-pc-windows-msvc/release/crypt-service.exe", executable: true },
        { role: "icon", name: "membrane-tab-icon.png", source: "install/assets/membrane-tab-icon.png" },
        { role: "license", name: "LICENSE", source: "LICENSE" },
        { role: "eula", name: "EULA.txt", source: "EULA.txt" },
        { role: "privacy", name: "PRIVACY.md", source: "PRIVACY.md" },
        { role: "third-party-notices", name: "THIRD-PARTY-NOTICES.txt", source: "THIRD-PARTY-NOTICES.txt" },
      ],
      build: {
        cmd: "rustup",
        args: ["run", "stable", "cargo", "build", "--manifest-path", "engine/Cargo.toml", "--workspace", "--locked", "--release", "--target", "x86_64-pc-windows-msvc", "-p", "membrane", "--bin", "membrane", "-p", "crypt", "--bin", "crypt-service"],
      },
      signing: { contract: "azure-artifact-signing-v1" },
    },
  },
};
