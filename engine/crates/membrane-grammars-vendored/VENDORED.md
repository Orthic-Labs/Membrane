# Vendored tree-sitter grammars

Each `grammars/<lang>/` directory is a vendored checkout of the upstream grammar
repository at the exact commit/tag that the legacy JS Blueprint's tree-sitter-wasms
bundle (`tree-sitter-wasms@0.1.13`, resolved 2026-09-10) pinned. Where the upstream
repo shipped a generated `src/parser.c` at an ABI tree-sitter 0.26 supports
(ABI 13-15), it is used unmodified. Where `src/parser.c` was missing (not committed
upstream) it was regenerated locally with `tree-sitter-cli 0.26.13`
(downloaded from the `tree-sitter/tree-sitter` v0.26.13 GitHub release, Windows x64
build, since npm/npx were unavailable in the lane's shell) via `tree-sitter generate`
run inside the grammar's own clone. No grammar in this set had an ABI < 13.

| lang | repo | ref | commit | parser.c source | notes |
|---|---|---|---|---|---|
| elisp | https://github.com/Wilfred/tree-sitter-elisp.git | 1.7.2 (npm `tree-sitter-elisp@^1.3.0` -> latest matching, 1.7.2) | tag 1.7.2 | regenerated (tree-sitter-cli 0.26.13, ABI 15) | upstream repo does not commit parser.c |
| embedded_template | https://github.com/tree-sitter/tree-sitter-embedded-template.git | v0.20.0 (npm `tree-sitter-embedded-template@^0.20.0`) | tag v0.20.0 | vendored as-is (ABI 14) | official tree-sitter org grammar for ERB/EJS |
| ql | https://github.com/samlanning/tree-sitter-ql.git | v1.0.0 (npm `tree-sitter-ql@^1.0.0`) | tag v1.0.0 | regenerated (tree-sitter-cli 0.26.13, ABI 14) | no LICENSE file upstream; package.json declares MIT |
| rescript | https://github.com/rescript-lang/tree-sitter-rescript.git | pinned by tree-sitter-wasms | commit `6376fa028f31aa4e26ca2c8f007e322cd2a5eb4a` | regenerated (tree-sitter-cli 0.26.13, ABI 14) | `grammar.js`'s `escape_sequence` rule used an unescaped `/u{[0-9a-fA-F]+}/` regex that tree-sitter-cli 0.26 rejects (braces parsed as a quantifier); fixed locally to `/u\{[0-9a-fA-F]+\}/` (semantically identical - literal brace match) before generating |
| solidity | https://github.com/JoranHonig/tree-sitter-solidity.git | pinned by tree-sitter-wasms | commit `b239a95f94cfcc6e7b3e961bc73a28d55e214f02` | vendored as-is (ABI 14) | |
| systemrdl | https://github.com/SystemRDL/tree-sitter-systemrdl.git | v0.7.0 (npm `tree-sitter-systemrdl@^0.7.0`) | commit `fb7e4134e478962fb70097b42a767364f4928a5e` (repo has no git tags; commit resolved via npm registry `gitHead` for the 0.7.0 publish) | regenerated (tree-sitter-cli 0.26.13, ABI 14) | |
| tlaplus | https://github.com/tlaplus-community/tree-sitter-tlaplus.git | v1.2.4 | tag v1.2.4 | vendored as-is (ABI 14) | npm scoped package `@tlaplus/tree-sitter-tlaplus` resolves to `^1.2.4` -> npm shows 1.5.0 as latest, but the git repo's tags stop at v1.2.4 (npm package version numbering diverged from git tags after v1.2.4); v1.2.4 is the highest tagged commit actually reachable in the source repo and is used as the parity source |
| vue | https://github.com/tree-sitter-grammars/tree-sitter-vue.git | pinned by tree-sitter-wasms | commit `7e48557b903a9db9c38cea3b7839ef7e1f36c693` | vendored as-is (ABI 14) | |

Each grammar directory carries its own `LICENSE` copy from upstream where the
upstream repo provided one (`ql` did not; it declares `"license": "MIT"` in its
`package.json` instead, recorded here for provenance).

Nested `.git` metadata is removed from every vendored grammar; all grammar
sources are ordinary files owned by this repository. `build.rs` compiles only
`src/parser.c` plus any `src/scanner.c`.

The vendored source contains two local generation fixes. ReScript's
`grammar.js` escape-sequence regex was corrected before generation as documented
in its table row. Elisp's `src/tree_sitter/array.h` also carries a local
strict-aliasing/array-macro compatibility patch required by its generated
parser; this patch is not an upstream commit.

`elm` is **not** vendored here — it is native via the
`tree-sitter-elm = "5.9.4"` crates.io crate directly in `membrane-blueprint`'s
`Cargo.toml`. The 27 direct parser crates provide 28 catalog languages because
`tree-sitter-typescript` supplies both TypeScript and TSX; eight additional
catalog languages use this vendored crate.
