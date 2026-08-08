# Windows release contract

MBR-902 ships `Membrane_<semver>_x64-setup.exe` (Tauri NSIS). Release manifest must bind installer path/hash to a 40-character source commit. Every PE payload and installer is signed with Azure Artifact Signing Public Trust using SHA-256 and an RFC3161 timestamp. Signing is performed outside this repository; `prepare-signing.mjs` emits an explicit AzureSignTool plan with placeholders and a SignTool `/pa /tw` verification command, never credentials or publication.

`verify-receipt.mjs` accepts only `windows-installer-receipt.v1`, status `pass`, matching source/hash, and passing `signature`, `install`, `update`, and `uninstall` gates. Missing or mismatched inputs fail closed. A passing source contract is not artifact acceptance: clean Windows receipts and signed installer bytes remain required.
