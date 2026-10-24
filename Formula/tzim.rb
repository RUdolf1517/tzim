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
  version "0.5.0"
  license "MIT"

  livecheck do
    url :homepage
    strategy :github_latest
  end

  # Only platforms with a published build. Homebrew on ARM Linux has no URL here
  # on purpose: `install.sh` and `cargo install --git` cover it, and a formula
  # that silently guessed a binary would be worse.
  on_macos do
    on_arm do
      url "https://github.com/RUdolf1517/tzim/releases/download/v0.5.0/tzim-aarch64-apple-darwin.tar.gz"
      sha256 "cd74295bbd152d0f2e04decef023a9e6b647ed2d6b4bff605f5d250913da215c"
    end
    on_intel do
      url "https://github.com/RUdolf1517/tzim/releases/download/v0.5.0/tzim-x86_64-apple-darwin.tar.gz"
      sha256 "687ec9f4fd43a7865467c6c3c7613ebefa94bcb6f2ef94ca734138e4263e395d"
    end
  end

  on_linux do
    on_intel do
      url "https://github.com/RUdolf1517/tzim/releases/download/v0.5.0/tzim-x86_64-unknown-linux-musl.tar.gz"
      sha256 "ccf76aa0097505c926b9c6283fc42ca0f2d69c1e482541479e4c125b5eb8aef4"
    end
  end

  def install
    bin.install "tzim"
  end

  def caveats
    <<~EOS
      Formats: PNG, APNG, gzip, the ZIP family, ICO/CUR, and baseline JPEG.
      JPEG is opt-in: tzim photo.jpg --formats jpeg

      Paths come first: tzim photo.png. `tzim -i` answers what this build is.

      No runtime dependencies. The binary is statically linked and opens no sockets.
      Run `tzim --list-formats` to see what this build supports.
    EOS
  end

  test do
    assert_match version.to_s, shell_output("#{bin}/tzim --version")
    assert_match "png", shell_output("#{bin}/tzim --list-formats")
  end
end
