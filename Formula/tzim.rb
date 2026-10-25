# Third-party tap. Homebrew 6+ asks for explicit trust for taps that are not
# official, so the two lines below are the whole install:
#
#   brew tap RUdolf1517/tzim https://github.com/RUdolf1517/tzim
#   brew install RUdolf1517/tzim/tzim
#
# A custom remote is required because the repository is not named
# homebrew-tzim. Short name afterwards:
#
#   brew trust --formula RUdolf1517/tzim/tzim && brew install tzim
#
# The tap builds nothing: it unpacks the published artifact, so the bytes are
# the ones listed in the release notes and in SHA256SUMS.
class Tzim < Formula
  desc "Lossless file optimizer: shrink the file, never the information"
  homepage "https://github.com/RUdolf1517/tzim"
  version "0.10.0"
  license "MIT"

  livecheck do
    url :homepage
    strategy :github_latest
  end

  # Only platforms with a published build. Homebrew on Intel macOS or ARM Linux
  # has no URL here on purpose: `install.sh` and `cargo install --git` cover
  # those, and a formula that silently guessed a binary would be worse.
  on_macos do
    on_arm do
      url "https://github.com/RUdolf1517/tzim/releases/download/v0.10.0/tzim-aarch64-apple-darwin.tar.gz"
      sha256 "9767e2b902decfed7cc864e686af3c46bc730538bf20dc4d35823b8288e74abf"
    end
  end

  on_linux do
    on_intel do
      url "https://github.com/RUdolf1517/tzim/releases/download/v0.10.0/tzim-x86_64-unknown-linux-musl.tar.gz"
      sha256 "1b10c4b2d93cd9ee76bc71fedfa560bf95ab5146f88fab37c4e13fd3b3882367"
    end
  end

  def install
    bin.install "tzim"
  end

  def caveats
    <<~EOS
      Formats: PNG, APNG, gzip, the ZIP family, ICO/CUR, baseline JPEG, TIFF and zlib.
      JPEG is opt-in: tzim photo.jpg --formats jpeg

      No runtime dependencies. The binary is statically linked and opens no sockets.
      Run `tzim --list-formats` to see what this build supports.
    EOS
  end

  test do
    assert_match version.to_s, shell_output("#{bin}/tzim --version")
    assert_match "png", shell_output("#{bin}/tzim --list-formats")
  end
end
