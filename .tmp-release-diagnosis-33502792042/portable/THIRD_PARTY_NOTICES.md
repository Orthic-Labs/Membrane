Third-Party Notices

Third-party software, models, fonts, data, and other components remain
subject to their own licenses. Those licenses control for those components and
are not restricted by the Orthic Labs Source Use License v1.0.

Scope: this package's complete locked JavaScript resolution is recorded in
`pnpm-lock.yaml`; its complete locked Rust registry and git resolution is
recorded in `engine/Cargo.lock`. These lockfiles, rather than only direct
dependencies from `package.json`, are the component inventories for this
source package. `dist/packaging/legal/verify.mjs` parses both inventories and fails
when either inventory is absent or this notice stops identifying it.

This notice does not classify every component's license or reproduce every
component's license text. The applicable license is each component's bundled
or upstream license. A released installer or platform bundle may include
additional components; its release process must produce and bind an artifact-
specific third-party inventory before distribution.
