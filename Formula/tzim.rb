# Third-party tap (Homebrew 6+ tap trust). A custom remote is required because
# this repository is not named homebrew-tzim, so `brew tap` needs the URL.
#
#   brew tap RUdolf1517/tzim https://github.com/RUdolf1517/tzim
#   brew install RUdolf1517/tzim/tzim
#
# Short name: brew trust --formula RUdolf1517/tzim/tzim && brew install tzim
#
# The tap builds nothing: it unpacks the published artifact, so the bytes are
# the ones listed in SHA256SUMS.
class Tzim < Formula
  desc "Lossless file optimizer: shrink the file, never the information"
  homepage "https://github.com/RUdolf1517/tzim"
  version "0.4.3"
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
      url "https://github.com/RUdolf1517/tzim/releases/download/v0.4.3/tzim-aarch64-apple-darwin.tar.gz"
      sha256 "0faae2e29112b08c66da5155ef771de1467bbd7a302bc8c28f38dc0fa76f3d3a"
    end
  end

  on_linux do
    on_intel do
      url "https://github.com/RUdolf1517/tzim/releases/download/v0.4.3/tzim-x86_64-unknown-linux-musl.tar.gz"
      sha256 "1b067deaccfb3be14739239a8522e8406ec8b560df647d9105b69b74789c8a79"
    end
  end

  def install
    bin.install "tzim"
  end

  def caveats
    <<~EOS
      Formats: PNG, APNG, gzip, the ZIP family, ICO/CUR, and baseline JPEG.
      JPEG is opt-in: tzim photo.jpg --formats jpeg

      No runtime dependencies. The binary is statically linked and opens no sockets.
      Run `tzim formats` to see what this build supports.
    EOS
  end

  test do
    assert_match version.to_s, shell_output("#{bin}/tzim --version")
    assert_match "png", shell_output("#{bin}/tzim formats")
  end
end
