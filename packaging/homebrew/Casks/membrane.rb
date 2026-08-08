cask "membrane" do
  raise <<~MESSAGE
    Membrane Homebrew Cask is intentionally unavailable.
    Generate it only from packaging/homebrew/release.v1.json after an immutable
    version, DMG URL, SHA-256, commit identity, codesign, notarization, & staple
    receipts exist for both Apple Silicon & Intel macOS.
  MESSAGE
end
