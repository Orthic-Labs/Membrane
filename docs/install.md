# Membrane install contract

> See also: [installation/](installation/) for the manifest/IPC handshake contract and stable-roots reference.

Membrane Hub owns desktop installation, runtime supervision, release builds,
updates, & install cleanup. Membrane's signed package supplies `membrane`,
`cortex`, Blueprint/Cortex contracts, icons, legal files, & release
evidence as one self-contained product.

For headless use, run installed `membrane supervisor-child`. No external
product manifest, add-on handoff, or retired installer lane is
active. Membrane Hub is the only desktop runtime/build/release/install
authority; its native macOS DMG lane is enabled only by signed release
evidence. Non-Mac targets are outside current implementation scope.
OCI metadata remains evaluation-only until independently published evidence
exists.
