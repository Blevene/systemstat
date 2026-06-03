# Homebrew formula TEMPLATE for systemstat.
#
# This is not yet usable: it needs the macOS (darwin) release tarballs produced
# once first-class macOS support lands (issue #2). Until then it documents the
# intended shape. After darwin artifacts exist, fill in the URLs/sha256 and host
# this in a tap (e.g. Blevene/homebrew-tap).
class Systemstat < Formula
  desc "Terminal system-monitor dashboard"
  homepage "https://github.com/Blevene/systemstat"
  version "0.0.1"

  on_macos do
    on_arm do
      url "https://github.com/Blevene/systemstat/releases/download/v0.0.1/systemstat-v0.0.1-aarch64-apple-darwin.tar.gz"
      sha256 "TODO_FILL_AFTER_DARWIN_BUILD"
    end
    on_intel do
      url "https://github.com/Blevene/systemstat/releases/download/v0.0.1/systemstat-v0.0.1-x86_64-apple-darwin.tar.gz"
      sha256 "TODO_FILL_AFTER_DARWIN_BUILD"
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
