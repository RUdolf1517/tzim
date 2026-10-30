//! Focused tests of the public, feature-dependent informational CLI.

use std::fs;
use std::process::{Command, Output, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

fn run(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_tzim"))
        .args(args)
        .output()
        .expect("run tzim")
}

#[test]
fn list_formats_describes_jpeg_admission_for_this_build() {
    let output = run(&["--list-formats"]);
    assert!(output.status.success(), "{output:?}");
    let stdout = String::from_utf8(output.stdout).expect("UTF-8 --list-formats output");

    #[cfg(feature = "jpeg")]
    {
        let jpeg = stdout
            .lines()
            .find(|line| line.starts_with("JPEG "))
            .expect("compiled JPEG row");
        if cfg!(feature = "jpeg-v1") {
            assert!(jpeg.contains("independent V1 verification"), "{jpeg}");
            assert!(jpeg.contains("default-enabled"), "{jpeg}");
            assert!(!jpeg.contains("opt-in via --formats"), "{jpeg}");
        } else {
            assert!(jpeg.contains("codec-only"), "{jpeg}");
            assert!(jpeg.contains("opt-in via --formats jpeg"), "{jpeg}");
            assert!(!jpeg.contains("default-enabled"), "{jpeg}");
        }
    }

    #[cfg(not(feature = "jpeg"))]
    assert!(!stdout.lines().any(|line| line.starts_with("JPEG ")), "{stdout}");
}

#[test]
fn capabilities_is_one_versioned_read_only_json_record() {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock after epoch")
        .as_nanos();
    let isolated = std::env::temp_dir().join(format!(
        "tzim-capabilities-{}-{nonce}",
        std::process::id()
    ));
    fs::create_dir(&isolated).expect("create empty capability working directory");
    let output = Command::new(env!("CARGO_BIN_EXE_tzim"))
        .arg("--capabilities")
        .current_dir(&isolated)
        .stdin(Stdio::null())
        .output()
        .expect("run capabilities with non-TTY stdin/stdout");
    assert!(output.status.success(), "{output:?}");
    assert!(output.stderr.is_empty(), "diagnostics on stderr: {output:?}");
    assert_eq!(fs::read_dir(&isolated).unwrap().count(), 0, "capabilities wrote into its working directory");
    fs::remove_dir(&isolated).expect("remove empty capability working directory");
    let stdout = String::from_utf8(output.stdout).expect("UTF-8 capabilities output");
    let lines: Vec<_> = stdout.lines().collect();
    assert_eq!(lines.len(), 1, "{stdout}");
    assert!(lines[0].starts_with("{\"type\":\"capabilities\""), "{stdout}");
    assert!(lines[0].contains("\"capabilities_schema_version\":1"), "{stdout}");
    assert!(lines[0].contains("\"result_schema_version\":2"), "{stdout}");
    assert!(lines[0].contains("\"compiled_formats\":["), "{stdout}");
    assert!(lines[0].contains("\"default_enabled_formats\":["), "{stdout}");
    assert!(lines[0].contains("\"selection_flags\":["), "{stdout}");
    assert!(lines[0].contains("\"handler_opt_ins\":["), "{stdout}");
    assert!(lines[0].contains("\"confirmation_policies\":["), "{stdout}");
    assert!(
        lines[0].contains("\"scans_files\":false,\"writes_files\":false,\"prompts\":false"),
        "{stdout}"
    );

    #[cfg(feature = "jpeg")]
    assert!(lines[0].contains("\"jpeg\",\"available\":true"), "{stdout}");
    #[cfg(not(feature = "jpeg"))]
    assert!(lines[0].contains("\"jpeg\",\"available\":false"), "{stdout}");

    #[cfg(feature = "jpeg-v1")]
    assert!(lines[0].contains("\"default_enabled\":true"), "{stdout}");
}
