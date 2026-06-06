# Homebrew formula for systemstat.
#
# Pinned to the v0.1.0 darwin release tarballs (URLs + sha256 filled in below).
# To publish: copy this file into a tap (e.g. Blevene/homebrew-tap) so users can
# `brew install blevene/tap/systemstat`. On each new release, bump `version`, the
# two URLs, and the two sha256 values (sha256sum the published darwin tarballs).
class Systemstat < Formula
  desc "Terminal system-monitor dashboard"
  homepage "https://github.com/Blevene/systemstat"
  version "0.1.0"

  on_macos do
    on_arm do
      url "https://github.com/Blevene/systemstat/releases/download/v0.1.0/systemstat-v0.1.0-aarch64-apple-darwin.tar.gz"
      sha256 "9314febc6e20bfcdfa059ba75085b374c4b68b77a537d22756d6266d4db9ff24"
    end
    on_intel do
      url "https://github.com/Blevene/systemstat/releases/download/v0.1.0/systemstat-v0.1.0-x86_64-apple-darwin.tar.gz"
      sha256 "adb58313aa9bbaea7610e54d13523b3f527a635cf17da4b07c4abd8ef6effbee"
    end
  end

  def install
    # The release tarball nests the binary under a top-level
    # systemstat-<version>-<target>/ directory, so don't assume it's at the root.
    bin.install Dir["*/systemstat"].first
  end

  test do
    assert_match "systemstat", shell_output("#{bin}/systemstat --version")
  end
end
