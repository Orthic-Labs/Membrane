# D19: Homebrew formula — installs the same portable archive (immutable URL +
# exact SHA-256 from release/catalog.json), plus completions and man page.

class Cortex < Formula
  desc "Local evidence-backed repository map"
  homepage "https://github.com/Orthic-Labs/Cortex#readme"
  url "https://github.com/Orthic-Labs/Cortex/releases/download/v__VERSION__/cortex-darwin-arm64.tar.gz"
  sha256 "__DARWIN_ARM64_SHA256__"
  license "SEE LICENSE IN LICENSE"
  version "__VERSION__"

  depends_on :macos

  def install
    bin.install "bin/cortex"
    bin.install "bin/cortex-mcp"
    lib.install Dir["lib/*"]
    prefix.install "app"
    prefix.install "LICENSE"
    prefix.install "THIRD_PARTY_NOTICES"
  end

  test do
    assert_match "Cortex", shell_output("#{bin}/cortex --help")
  end
end
