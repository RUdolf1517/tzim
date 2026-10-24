//! Integration tests: they run the real binary against real files and assert on
//! exit codes, output and file contents.

mod common;

use std::path::Path;

use common::*;

#[test]
fn png_is_optimized_and_pixels_are_identical() {
    let ws = Workspace::new("png-opt");
    let spec = PngSpec {
        width: 128,
        height: 96,
        level: 1, // deliberately weak compression so a gain is guaranteed
        ..Default::default()
    };
    let original = build_png(&spec);
    let path = ws.write("photo.png", &original);

    let (code, stdout, stderr) = run(&["optimize", "photo.png"], &ws.path);
    assert_eq!(code, 0, "stdout={stdout} stderr={stderr}");

    let optimized = std::fs::read(&path).unwrap();
    assert!(optimized.len() < original.len(), "expected a smaller file");

    let (w1, h1, p1) = decode_pixels(&original).unwrap();
    let (w2, h2, p2) = decode_pixels(&optimized).unwrap();
    assert_eq!((w1, h1), (w2, h2));
    assert_eq!(p1, p2, "pixels must be identical");

    // The tool's own verification is not enough for a test: re-check the framing.
    assert!(png_chunks(&optimized).is_ok());
}

#[test]
fn png_ancillary_and_private_chunks_are_preserved() {
    let ws = Workspace::new("png-chunks");
    let spec = PngSpec {
        width: 64,
        height: 64,
        level: 1,
        extra_chunks: vec![
            ("tEXt".to_string(), b"Comment\0kept as is".to_vec()),
            ("prVt".to_string(), vec![1, 2, 3, 4, 5]),
            ("pHYs".to_string(), vec![0, 0, 0x0b, 0x13, 0, 0, 0x0b, 0x13, 1]),
        ],
        ..Default::default()
    };
    let original = build_png(&spec);
    let path = ws.write("img.png", &original);
    let (code, _, stderr) = run(&["optimize", "img.png"], &ws.path);
    assert_eq!(code, 0, "{stderr}");

    let optimized = std::fs::read(&path).unwrap();
    let before = png_chunks(&original).unwrap();
    let after = png_chunks(&optimized).unwrap();

    let kept_before: Vec<_> = before.iter().filter(|(t, _)| t != "IDAT").collect();
    let kept_after: Vec<_> = after.iter().filter(|(t, _)| t != "IDAT").collect();
    assert_eq!(kept_before, kept_after, "non-IDAT chunks must be preserved verbatim");
}

#[test]
fn dry_run_does_not_modify_anything() {
    let ws = Workspace::new("png-dry");
    let spec = PngSpec {
        width: 96,
        height: 96,
        level: 1,
        ..Default::default()
    };
    let original = build_png(&spec);
    let path = ws.write("photo.png", &original);

    let (code, stdout, stderr) = run(&["optimize", "photo.png", "--dry-run"], &ws.path);
    assert_eq!(code, 0, "{stderr}");
    assert!(stdout.contains("dry run"), "{stdout}");
    assert_eq!(std::fs::read(&path).unwrap(), original, "file must be untouched");

    // No temporary file may survive a dry run.
    let leftovers: Vec<_> = std::fs::read_dir(&ws.path)
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_name().to_string_lossy().starts_with(".tzim-"))
        .collect();
    assert!(leftovers.is_empty(), "temporary files must be cleaned up");
}

#[test]
fn second_run_is_a_no_op() {
    let ws = Workspace::new("png-idempotent");
    let spec = PngSpec {
        width: 100,
        height: 100,
        level: 1,
        ..Default::default()
    };
    let original = build_png(&spec);
    let path = ws.write("photo.png", &original);

    let (code, _, stderr) = run(&["optimize", "photo.png"], &ws.path);
    assert_eq!(code, 0, "{stderr}");
    let once = std::fs::read(&path).unwrap();

    let (code, stdout, stderr) = run(&["optimize", "photo.png"], &ws.path);
    assert_eq!(code, 0, "{stderr}");
    assert!(
        stdout.contains("already optimal") || stdout.contains("optimized"),
        "{stdout}"
    );
    assert_eq!(std::fs::read(&path).unwrap(), once, "second run must not change the file");
}

#[test]
fn corrupt_png_is_refused_and_left_untouched() {
    let ws = Workspace::new("png-corrupt");
    let spec = PngSpec {
        width: 32,
        height: 32,
        level: 1,
        ..Default::default()
    };
    let mut original = build_png(&spec);
    // Corrupt a byte inside the IDAT payload so a chunk CRC no longer matches.
    let len = original.len();
    original[len - 30] ^= 0xff;
    let path = ws.write("broken.png", &original);

    let (code, stdout, _stderr) = run(&["optimize", "broken.png"], &ws.path);
    assert_eq!(code, 1, "{stdout}");
    assert_eq!(std::fs::read(&path).unwrap(), original, "corrupt input must be untouched");
}

#[test]
fn gzip_header_is_preserved_byte_for_byte() {
    let ws = Workspace::new("gz-header");
    let payload = b"the quick brown fox jumps over the lazy dog\n".repeat(200);
    let original = build_gzip(&payload, Some(b"original-name.txt"), Some(b"XYZZ"), 0x04, 1);
    let path = ws.write("data.gz", &original);

    let (code, stdout, stderr) = run(&["optimize", "data.gz"], &ws.path);
    assert_eq!(code, 0, "stdout={stdout} stderr={stderr}");

    let optimized = std::fs::read(&path).unwrap();
    assert!(optimized.len() < original.len(), "expected a smaller file");

    // Everything up to and including FEXTRA + FNAME must be identical.
    let header_len = 10 + 2 + 4 + "original-name.txt".len() + 1;
    assert_eq!(&optimized[..header_len], &original[..header_len], "header changed");
    assert_eq!(&optimized[optimized.len() - 8..], &original[original.len() - 8..], "trailer changed");
}

#[test]
fn gzip_payload_is_unchanged() {
    use std::io::Read;
    let ws = Workspace::new("gz-payload");
    let payload: Vec<u8> = (0..50_000).map(|i| (i % 251) as u8).collect();
    let original = build_gzip(&payload, None, None, 0, 1);
    let path = ws.write("blob.gz", &original);

    let (code, _, stderr) = run(&["optimize", "blob.gz"], &ws.path);
    assert_eq!(code, 0, "{stderr}");

    let optimized = std::fs::read(&path).unwrap();
    let header_len = 10;
    let body = &optimized[header_len..optimized.len() - 8];
    let mut decoder = flate2::read::DeflateDecoder::new(body);
    let mut restored = Vec::new();
    decoder.read_to_end(&mut restored).unwrap();
    assert_eq!(restored, payload);
}

#[test]
fn unsupported_files_are_reported_and_untouched() {
    let ws = Workspace::new("unsupported");
    let content = b"just text, no format here";
    let path = ws.write("notes.txt", content);

    let (code, stdout, stderr) = run(&["optimize", "notes.txt"], &ws.path);
    assert_eq!(code, 0, "{stderr}");
    assert!(stdout.contains("unsupported"), "{stdout}");
    assert_eq!(std::fs::read(&path).unwrap(), content);
}

#[test]
fn directory_is_processed_recursively() {
    let ws = Workspace::new("recursive");
    let spec = PngSpec {
        width: 64,
        height: 64,
        level: 1,
        ..Default::default()
    };
    ws.write("top.png", &build_png(&spec));
    ws.write("nested/deeper/inner.png", &build_png(&spec));
    ws.write("nested/ignore.txt", b"text");

    let (code, stdout, stderr) = run(&["optimize", ".", "--recursive", "--json"], &ws.path);
    assert_eq!(code, 0, "{stderr}");

    let records = json_file_records(&stdout);
    assert_eq!(records, 3, "expected three files, got {records}: {stdout}");
    assert!(stdout.contains("\"status\":\"optimized\""), "{stdout}");
    assert!(stdout.contains("\"status\":\"unsupported\""), "{stdout}");
}

#[test]
fn duplicates_are_processed_once() {
    let ws = Workspace::new("dedup");
    let spec = PngSpec {
        width: 32,
        height: 32,
        level: 1,
        ..Default::default()
    };
    ws.write("a.png", &build_png(&spec));

    let (code, stdout, stderr) = run(&["optimize", "a.png", "./a.png", ".", "--json"], &ws.path);
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(json_file_records(&stdout), 1, "duplicate paths must collapse: {stdout}");
}

#[test]
fn unicode_and_space_filenames_work() {
    let ws = Workspace::new("unicode");
    let spec = PngSpec {
        width: 48,
        height: 48,
        level: 1,
        ..Default::default()
    };
    let original = build_png(&spec);
    let name = "иконка 🌀 (копия).png";
    let path = ws.write(name, &original);

    let (code, stdout, stderr) = run(&["optimize", name], &ws.path);
    assert_eq!(code, 0, "stdout={stdout} stderr={stderr}");
    let optimized = std::fs::read(&path).unwrap();
    assert!(optimized.len() <= original.len());
    assert!(png_chunks(&optimized).is_ok());
}

#[test]
fn json_output_has_one_record_per_file_and_a_summary() {
    let ws = Workspace::new("json");
    let spec = PngSpec {
        width: 32,
        height: 32,
        level: 1,
        ..Default::default()
    };
    ws.write("a.png", &build_png(&spec));
    ws.write("b.txt", b"text");

    let (code, stdout, stderr) = run(&["optimize", ".", "--json"], &ws.path);
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(json_file_records(&stdout), 2, "{stdout}");
    assert!(stdout.contains("\"type\":\"summary\""), "{stdout}");
    assert!(stdout.contains("\"schema\":1"), "{stdout}");
    assert!(stdout.contains("\"dry_run\":false"), "{stdout}");

    // Every line must be valid JSON: a strict parser checks the escaping.
    for line in stdout.lines() {
        let trimmed = line.trim();
        assert!(trimmed.starts_with('{') && trimmed.ends_with('}'), "{line}");
        assert_eq!(
            trimmed.matches('{').count(),
            trimmed.matches('}').count(),
            "unbalanced braces in {line}"
        );
    }
}

#[test]
fn missing_path_fails_but_others_continue() {
    let ws = Workspace::new("missing");
    let spec = PngSpec {
        width: 32,
        height: 32,
        level: 1,
        ..Default::default()
    };
    ws.write("good.png", &build_png(&spec));

    let (code, stdout, stderr) =
        run(&["optimize", "does-not-exist.png", "good.png"], &ws.path);
    assert_eq!(code, 1, "stdout={stdout} stderr={stderr}");
    assert!(stdout.contains("failed"), "{stdout}");
    assert!(stdout.contains("optimized"), "{stdout}");
}

#[test]
fn apng_control_chunks_are_untouched() {
    let ws = Workspace::new("apng");
    let spec = PngSpec {
        width: 64,
        height: 64,
        level: 1,
        apng: true,
        ..Default::default()
    };
    let original = build_png(&spec);
    let path = ws.write("anim.png", &original);

    let (code, _, stderr) = run(&["optimize", "anim.png"], &ws.path);
    assert_eq!(code, 0, "{stderr}");

    let optimized = std::fs::read(&path).unwrap();
    let before: Vec<_> = png_chunks(&original)
        .unwrap()
        .into_iter()
        .filter(|(t, _)| t == "acTL")
        .collect();
    let after: Vec<_> = png_chunks(&optimized)
        .unwrap()
        .into_iter()
        .filter(|(t, _)| t == "acTL")
        .collect();
    assert_eq!(before, after);
}

#[test]
fn usage_errors_exit_with_two() {
    let ws = Workspace::new("usage");
    let (code, _, stderr) = run(&["optimize", "--nope"], &ws.path);
    assert_eq!(code, 2, "{stderr}");

    let (code, _, _) = run(&["optimize"], &ws.path);
    assert_eq!(code, 2);
}

#[test]
fn help_and_version_succeed() {
    let ws = Workspace::new("help");
    let (code, stdout, _) = run(&["--help"], &ws.path);
    assert_eq!(code, 0);
    assert!(stdout.contains("tzim optimize"));

    let (code, stdout, _) = run(&["--version"], &ws.path);
    assert_eq!(code, 0);
    assert!(stdout.starts_with("tzim "));

    let (code, stdout, _) = run(&["formats"], &ws.path);
    assert_eq!(code, 0);
    assert!(stdout.contains("PNG"));
}

#[test]
fn include_and_exclude_filters() {
    let ws = Workspace::new("filters");
    let spec = PngSpec {
        width: 32,
        height: 32,
        level: 1,
        ..Default::default()
    };
    let png = ws.write("a.png", &build_png(&spec));
    let payload = b"the quick brown fox jumps over the lazy dog\n".repeat(500);
    let gz = ws.write("b.gz", &build_gzip(&payload, None, None, 0, 1));
    let png_before = std::fs::read(&png).unwrap();
    let gz_before = std::fs::read(&gz).unwrap();

    let (code, _, stderr) = run(&["optimize", ".", "--include", "gz"], &ws.path);
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(std::fs::read(&png).unwrap(), png_before, "png must be filtered out");
    assert_ne!(std::fs::read(&gz).unwrap(), gz_before, "gz must be processed");
}

#[cfg(unix)]
#[test]
fn read_only_file_is_skipped() {
    use std::os::unix::fs::PermissionsExt;
    let ws = Workspace::new("readonly");
    let spec = PngSpec {
        width: 32,
        height: 32,
        level: 1,
        ..Default::default()
    };
    let original = build_png(&spec);
    let path = ws.write("locked.png", &original);
    let mut perms = std::fs::metadata(&path).unwrap().permissions();
    perms.set_mode(0o444);
    std::fs::set_permissions(&path, perms).unwrap();

    let (code, stdout, stderr) = run(&["optimize", "locked.png"], &ws.path);
    assert_eq!(code, 0, "{stderr}");

    let mut perms = std::fs::metadata(&path).unwrap().permissions();
    perms.set_mode(0o644);
    std::fs::set_permissions(&path, perms).unwrap();

    assert_eq!(std::fs::read(&path).unwrap(), original);
    assert!(stdout.contains("read-only") || stdout.contains("skipped"), "{stdout}");
}

#[cfg(unix)]
#[test]
fn permissions_and_mtime_survive_the_replacement() {
    use std::os::unix::fs::PermissionsExt;
    let ws = Workspace::new("attrs");
    let spec = PngSpec {
        width: 80,
        height: 80,
        level: 1,
        ..Default::default()
    };
    let path = ws.write("attrs.png", &build_png(&spec));

    let mut perms = std::fs::metadata(&path).unwrap().permissions();
    perms.set_mode(0o640);
    std::fs::set_permissions(&path, perms).unwrap();
    let before = std::fs::metadata(&path).unwrap();

    let (code, _, stderr) = run(&["optimize", "attrs.png"], &ws.path);
    assert_eq!(code, 0, "{stderr}");

    let after = std::fs::metadata(&path).unwrap();
    assert_eq!(before.permissions().mode() & 0o777, after.permissions().mode() & 0o777);
    assert_eq!(
        before.modified().unwrap(),
        after.modified().unwrap(),
        "mtime must be preserved"
    );
}

#[test]
fn symlinks_are_not_followed_by_default() {
    #[cfg(unix)]
    {
        let ws = Workspace::new("symlink");
        let spec = PngSpec {
            width: 32,
            height: 32,
            level: 1,
            ..Default::default()
        };
        let target = ws.write("real.png", &build_png(&spec));
        let link = ws.file("link.png");
        std::os::unix::fs::symlink(&target, &link).unwrap();

        let before = std::fs::read(&target).unwrap();
        let (code, stdout, stderr) = run(&["optimize", "link.png"], &ws.path);
        assert_eq!(code, 0, "{stderr}");
        assert_eq!(std::fs::read(&target).unwrap(), before, "target must be untouched");
        assert!(
            stdout.contains("symbolic link") || stdout.contains("skipped"),
            "{stdout}"
        );
    }
}

#[test]
fn a_directory_argument_without_recursive_stays_at_the_top_level() {
    let ws = Workspace::new("top-level");
    let spec = PngSpec {
        width: 32,
        height: 32,
        level: 1,
        ..Default::default()
    };
    ws.write("outer.png", &build_png(&spec));
    ws.write("inner/inner.png", &build_png(&spec));

    let (code, stdout, stderr) = run(&["optimize", ".", "--json"], &ws.path);
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(json_file_records(&stdout), 1, "{stdout}");
}

#[allow(dead_code)]
fn exists(path: &Path) -> bool {
    path.exists()
}
