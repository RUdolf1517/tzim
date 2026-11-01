//! The files that are shipped but never compiled: the manual page, the shell
//! completions, and the two scripts that decide what a release contains.
//!
//! They cannot be tested by running the binary, and they are exactly the kind of
//! file that quietly drifts out of date — a flag added to the parser, a target
//! added to the release matrix, and a man page that still describes the previous
//! year. So the drift itself is the assertion: everything the program says about
//! itself in `--help` has to appear in every one of them.

use std::collections::BTreeSet;
use std::fs;

fn read(path: &str) -> String {
    let full = format!("{}/{}", env!("CARGO_MANIFEST_DIR"), path);
    fs::read_to_string(&full).unwrap_or_else(|e| panic!("cannot read {path}: {e}"))
}

/// Every long option mentioned in `--help`, in order of first appearance.
///
/// `--color=never` is written inside the help of `--no-color`, and a naive token
/// split would invent an option called `--colornever`; everything from `=` on is
/// therefore dropped before the name is validated.
fn help_options() -> Vec<String> {
    let mut seen = BTreeSet::new();
    let mut out = Vec::new();
    for token in tzim::cli::HELP.split(|c: char| c.is_whitespace() || c == ',' || c == '`') {
        let name = token.split('=').next().unwrap_or(token);
        let name: String = name
            .trim_end_matches(|c: char| !(c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-'))
            .to_string();
        let valid = name.len() > 3
            && name.starts_with("--")
            && name[2..]
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
        if valid && seen.insert(name.clone()) {
            out.push(name);
        }
    }
    out
}

/// roff spells a literal hyphen as `\-`, so the page is compared in the text a
/// reader sees rather than in the source a formatter reads.
fn man_text() -> String {
    read("man/tzim.1").replace("\\-", "-")
}

#[test]
fn help_lists_the_metadata_flag() {
    // It was accepted by the parser and missing from --help from v0.6 to v0.10.
    let flags = help_options();
    assert!(
        flags.iter().any(|o| o == "--strip-metadata"),
        "the flag exists in the parser and must exist in --help"
    );
}

#[test]
fn the_manual_page_documents_every_option() {
    let man = man_text();
    let flags = help_options();
    let missing: Vec<&String> = flags
        .iter()
        .filter(|flag| !man.contains(flag.as_str()))
        .collect();
    assert!(missing.is_empty(), "missing from tzim.1: {missing:?}");
}

#[test]
fn the_manual_page_is_for_this_version() {
    let man = man_text();
    let header = man
        .lines()
        .find(|line| line.starts_with(".TH"))
        .expect("a .TH header");
    assert!(
        header.contains(env!("CARGO_PKG_VERSION")),
        "tzim.1 says {header:?}, the crate is {}",
        env!("CARGO_PKG_VERSION")
    );
    assert!(
        header.contains(" TZIM 1 "),
        "the page is section 1: {header:?}"
    );
    // A man page without a NAME line is not installed by `man`.
    assert!(man.contains(".SH NAME"));
    assert!(man.contains(".SH SYNOPSIS"));
}

#[test]
fn every_completion_knows_every_option() {
    let flags = help_options();
    for file in ["completions/tzim.bash", "completions/_tzim"] {
        let body = read(file);
        let missing: Vec<&String> = flags
            .iter()
            .filter(|flag| !body.contains(flag.as_str()))
            .collect();
        assert!(missing.is_empty(), "missing from {file}: {missing:?}");
    }

    // fish spells a long option without the leading dashes: `-l recursive`.
    let fish = read("completions/tzim.fish");
    let missing: Vec<&String> = flags
        .iter()
        .filter(|flag| !fish.contains(&format!("-l {}", &flag[2..])))
        .collect();
    assert!(
        missing.is_empty(),
        "missing from completions/tzim.fish: {missing:?}"
    );
}

#[test]
fn every_release_target_is_installable() {
    // The matrix decides what is published; install.sh decides what is
    // downloaded. A target added to one and not the other is a platform that
    // silently gets the cargo fallback, or a formula pointing at nothing.
    let workflow = read(".github/workflows/release.yml");
    let installer = read("install.sh");
    let targets: Vec<String> = workflow
        .lines()
        .filter_map(|line| line.trim().strip_prefix("target: "))
        .map(|t| t.trim().to_string())
        .collect();
    assert!(targets.len() >= 4, "the matrix looks empty: {targets:?}");
    for target in &targets {
        // Windows is a zip, not a shell installer.
        if target.contains("windows") {
            continue;
        }
        assert!(
            installer.contains(target.as_str()),
            "release.yml builds {target} and install.sh cannot fetch it"
        );
    }
}

#[test]
fn the_linux_packages_carry_the_documentation() {
    let script = read("packaging/linux-packages.sh");
    // The directory is a variable in the script, so the files are matched by
    // name; `man/tzim.1` is spelled out because the fallback for an older
    // archive uses the checkout's copy.
    for asset in ["man/tzim.1", "tzim.bash", "_tzim", "tzim.fish"] {
        assert!(
            script.contains(asset),
            "the .deb and .rpm must ship {asset}"
        );
    }
    // Both architectures, because both binaries are built now.
    assert!(script.contains("arm64") && script.contains("amd64"));
}

#[test]
fn the_formula_installs_more_than_a_binary() {
    let formula = read("Formula/tzim.rb");
    assert!(formula.contains("man1.install"));
    assert!(formula.contains("bash_completion"));
    assert!(formula.contains("zsh_completion"));
    assert!(formula.contains("fish_completion"));
}

#[test]
fn the_windows_installer_is_checksum_pinned_per_user_and_release_wired() {
    let installer = read("packaging/windows/install.ps1");
    assert!(
        installer.starts_with('\u{feff}'),
        "PowerShell installer must retain its UTF-8 BOM"
    );
    let integration_harness = read("packaging/windows/tests/Install.Integration.ps1");
    assert!(
        integration_harness.starts_with('\u{feff}'),
        "PowerShell integration harness must retain its UTF-8 BOM"
    );
    for required in [
        "api.github.com/repos/",
        "releases/download/",
        "tzim-x86_64-pc-windows-msvc.zip",
        "SHA256SUMS",
        "Get-FileHash",
        "LOCALAPPDATA",
        "EnvironmentVariableTarget]::User",
        "path_entry_owned",
        "Invoke-TzimUninstallCore",
        "Assert-TzimExecutableAvailable",
        "Assert-TzimX64Architecture",
        "Unsupported Windows architecture",
    ] {
        assert!(
            installer.contains(required),
            "Windows installer is missing {required}"
        );
    }
    assert!(!installer.contains("Invoke-Expression"));
    assert!(!installer.contains("RunAs"));
    assert!(!installer.contains("EnvironmentVariableTarget]::Machine"));
    assert!(!installer.contains("microsoft/winget-pkgs"));
    assert!(installer.contains("TZIM_GITHUB_TOKEN"));

    let integration_workflow = read(".github/workflows/windows-installer.yml");
    assert!(integration_workflow.contains("TZIM_GITHUB_TOKEN: ${{ secrets.GITHUB_TOKEN }}"));

    let workflow = read(".github/workflows/release.yml");
    assert!(workflow.contains("cp packaging/windows/install.ps1 artifacts/install.ps1"));
    assert!(workflow.contains("packaging/winget/generate.py"));
    assert!(workflow.contains("sha256sum ./* > SHA256SUMS"));

    let generator = read("packaging/winget/generate.py");
    for filename in [
        "RUdolf1517.tzim.yaml",
        "RUdolf1517.tzim.installer.yaml",
        "RUdolf1517.tzim.locale.en-US.yaml",
    ] {
        assert!(
            generator.contains(filename),
            "local winget manifest filename is not canonical: {filename}"
        );
    }
    assert!(!generator.contains("microsoft/winget-pkgs"));

    let manifest_template = read("packaging/winget/templates/installer.yaml.in");
    for field in [
        "Architecture: x64",
        "InstallerSha256:",
        "NestedInstallerType: portable",
        "RelativeFilePath: tzim.exe",
        "PortableCommandAlias: tzim",
    ] {
        assert!(
            manifest_template.contains(field),
            "winget template is missing {field}"
        );
    }
    assert!(
        !manifest_template.contains("Scope:"),
        "portable installer manifests do not support the Scope field"
    );

    let default_locale_template = read("packaging/winget/templates/locale.en-US.yaml.in");
    assert!(
        default_locale_template.contains("ManifestType: defaultLocale"),
        "the en-US local manifest must be typed as the version manifest's default locale"
    );
}

#[test]
fn the_installer_verifies_what_it_downloads() {
    let script = read("install.sh");
    assert!(
        script.contains("SHA256SUMS"),
        "install.sh must fetch the checksums"
    );
    assert!(
        script.contains("sha256sum") && script.contains("shasum"),
        "both Linux and macOS tools must be supported"
    );
    // A checksum that does not match is a hard stop, not a reason to build from
    // source and hope.
    assert!(script.contains("SHA-256 mismatch"));
}
