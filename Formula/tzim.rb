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
  version "0.16.0"
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
      url "https://github.com/RUdolf1517/tzim/releases/download/v0.16.0/tzim-aarch64-apple-darwin.tar.gz"
      sha256 "29da3142910fe08a01a00df939ea659f94e928b7ec8a13e6224640a426f5878e"
    end
    on_intel do
      url "https://github.com/RUdolf1517/tzim/releases/download/v0.16.0/tzim-x86_64-apple-darwin.tar.gz"
      sha256 "4b7802f5add9a946a4f48260bafbebaeafa1e8497fba431270b88ecb99921f3f"
    end
  end

  on_linux do
    on_intel do
      url "https://github.com/RUdolf1517/tzim/releases/download/v0.16.0/tzim-x86_64-unknown-linux-musl.tar.gz"
      sha256 "5a4cc1c6fe0d19f81f66744f47d0fd3ef352546e33828b1a6c1c765350c221c7"
    end
    on_arm do
      url "https://github.com/RUdolf1517/tzim/releases/download/v0.16.0/tzim-aarch64-unknown-linux-musl.tar.gz"
      sha256 "b9db40ec0270b96a0ad624569c0dd4cf28918bb23be37586e22bb200024edc09"
    end
  end

  # Archives from v0.11.0 onwards ship the manual page and the three
  # completions next to the binary, so the formula installs the same
  # documentation the .deb, the .rpm and the repository carry. The guards keep
  # older pinned archives installable; the v0.16.0 archives include all of these.
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
      Default handlers: PNG/APNG, gzip, ZIP, ICO/CUR, eligible baseline sequential JPEG,
      TIFF and zlib. Baseline sequential JPEG is enabled by default and independently
      checked with V1 pixel verification. `--formats` is an allowlist for narrowing
      handlers, not a JPEG opt-in; for example, `--formats png` excludes JPEG.
      Progressive JPEG is not supported.

      FLAC and PDF require non-default build features and are absent from this binary.
      FITS is also non-default; it requires a build with the `fits` feature and the
      `--fits-tile` run option, so it is absent from this Homebrew binary.

      No runtime dependencies. The binary is statically linked and opens no sockets.
      Run `tzim --list-formats` to see the handlers compiled into this build.
    EOS
  end

  test do
    assert_match version.to_s, shell_output("#{bin}/tzim --version")
    formats = shell_output("#{bin}/tzim --list-formats")
    rows = formats.lines.drop(1).take_while { |line| !line.strip.empty? }
    expected = %w[PNG gzip ICO/CUR JPEG TIFF ZIP ZLIB]
    actual = rows.map { |line| line.split.first }
    assert_equal expected, actual
    refute_match(/^\s*(?:FLAC|PDF|FITS)\s/, formats)
  end
end
