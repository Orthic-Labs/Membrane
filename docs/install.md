# Membrane install contract

> See also: [installation/](installation/) for the manifest/IPC handshake contract and stable-roots reference.

Orthic owns desktop installation. Membrane's portable add-on supplies only
the signed `membrane` command, `cortex-service`, icon, legal files, and sealed
manifest; it is adopted by Orthic before Orthic packages its own installer.

For headless use, run the installed `membrane service run` command. No
Homebrew, WinGet, Scoop, DMG, NSIS, or Membrane-local updater lane is active.
OCI metadata remains evaluation-only until independently published evidence
exists.
