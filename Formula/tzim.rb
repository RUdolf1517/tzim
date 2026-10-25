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
  version "0.13.0"
  license "MIT"

  livecheck do
    url :homepage
    strategy :github_latest
  end

  # One URL per published build. Homebrew on any other platform has no URL here
  # on purpose: `install.sh` and `cargo install --git` cover those, and a
  # formula that silently guessed a binary would be worse.
  on_macos do
    on_arm do
      url "https://github.com/RUdolf1517/tzim/releases/download/v0.13.0/tzim-aarch64-apple-darwin.tar.gz"
      sha256 "11c126848b53bad1a6f0caa406d4141c78df30051b74b8f952348ecdd210d21e"
    end
    on_intel do
      url "https://github.com/RUdolf1517/tzim/releases/download/v0.13.0/tzim-x86_64-apple-darwin.tar.gz"
      sha256 "0d36d70dd52d8bc049ca090c944ddc22b70b8e98cf2c2357758d2ca32c46d59b"
    end
  end

  on_linux do
    on_intel do
      url "https://github.com/RUdolf1517/tzim/releases/download/v0.13.0/tzim-x86_64-unknown-linux-musl.tar.gz"
      sha256 "44d818aba7403f6cd6a5f4b85fe70653e938caf3bea204dd3b3c0d466935bfb6"
    end
    on_arm do
      url "https://github.com/RUdolf1517/tzim/releases/download/v0.13.0/tzim-aarch64-unknown-linux-musl.tar.gz"
      sha256 "c51d4321e305a0ae9f477b150a7d3e7308da0dca2a4336c0d0cec34fedfcb126"
    end
  end

  # Archives from v0.11.0 onwards ship the manual page and the three
  # completions next to the binary, so the formula installs the same
  # documentation the .deb, the .rpm and the repository carry. The `File.exist?`
  # checks are for the pinned version only: while this formula points at an
  # older archive, those files are simply not there yet, and a formula that
  # refuses to install is worse than one without a man page.
  def install
    bin.install "tzim"
    man1.install "man/tzim.1" if File.exist?("man/tzim.1")
    if File.exist?("completions/tzim.bash")
      bash_completion.install "completions/tzim.bash" => "tzim"
      zsh_completion.install "completions/_tzim"
      fish_completion.install "completions/tzim.fish"
    end
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
