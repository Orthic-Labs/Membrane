# Membrane install contract

> See also: [installation/](installation/) for the manifest/IPC handshake contract and stable-roots reference.

Membrane Hub owns desktop activation, runtime supervision, release builds,
updates, & install cleanup. Primary distribution is signed native archives plus
`checksums.json` and bootstrap scripts hosted on GitHub Releases. Windows ships
`membrane-windows-x64.zip` and `install.ps1`; bootstrap verifies checksum and
Authenticode, stages a rollback-safe user-local swap, adds Membrane to user
`PATH`, runs `membrane activate`, then verifies exact resident generation.

The installed Hub hosts the only Membrane runtime. Headless clients are
stateless and do not create a resident service. No external product manifest,
add-on handoff, or retired installer lane is
active. Membrane Hub is the only desktop runtime/build/release/activation
authority. Setup EXE, MSI, DMG, WinGet, and Homebrew are optional channels, not
primary distribution or activation prerequisites. Membrane is not installed as
a Windows Service.
OCI metadata remains evaluation-only until independently published evidence
exists.
