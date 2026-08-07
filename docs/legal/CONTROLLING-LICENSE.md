# Controlling license

Every source and binary artifact in this repository is governed by **one controlling license**: the **Orthic Labs Source Use License v1.0** in the repository-root [`LICENSE`](../../LICENSE) file (Copyright © 2026 Damned Ventures LLC. All rights reserved.).

Because that license is proprietary (not an OSI/SPDX identifier), each package manifest names it by reference rather than by SPDX code:

- **JavaScript** — `package.json` sets `"license": "SEE LICENSE IN LICENSE"`.
- **Rust engine** — `engine/Cargo.toml` sets `workspace.package.license-file = "../LICENSE"`, and every member crate (`engine/crates/*/Cargo.toml`) inherits it via `license-file.workspace = true`. `cargo metadata --no-deps` resolves each crate's `license_file` to the repository-root `LICENSE`.

Root companion files `EULA.txt`, `PRIVACY.md`, and `THIRD-PARTY-NOTICES.txt` expose distribution-facing legal metadata. They add no license grant or restriction: `LICENSE` remains the sole controlling agreement. Release packaging must bind those companion files plus an artifact-specific dependency inventory before distribution.
