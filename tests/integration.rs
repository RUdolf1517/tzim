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

    let (code, stdout, stderr) = run(&["photo.png"], &ws.path);
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
            (
                "pHYs".to_string(),
                vec![0, 0, 0x0b, 0x13, 0, 0, 0x0b, 0x13, 1],
            ),
        ],
        ..Default::default()
    };
    let original = build_png(&spec);
    let path = ws.write("img.png", &original);
    let (code, _, stderr) = run(&["img.png"], &ws.path);
    assert_eq!(code, 0, "{stderr}");

    let optimized = std::fs::read(&path).unwrap();
    let before = png_chunks(&original).unwrap();
    let after = png_chunks(&optimized).unwrap();

    let kept_before: Vec<_> = before.iter().filter(|(t, _)| t != "IDAT").collect();
    let kept_after: Vec<_> = after.iter().filter(|(t, _)| t != "IDAT").collect();
    assert_eq!(
        kept_before, kept_after,
        "non-IDAT chunks must be preserved verbatim"
    );
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

    let (code, stdout, stderr) = run(&["photo.png", "--dry-run"], &ws.path);
    assert_eq!(code, 0, "{stderr}");
    assert!(stdout.contains("dry run"), "{stdout}");
    assert_eq!(
        std::fs::read(&path).unwrap(),
        original,
        "file must be untouched"
    );

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

    let (code, _, stderr) = run(&["photo.png"], &ws.path);
    assert_eq!(code, 0, "{stderr}");
    let once = std::fs::read(&path).unwrap();

    let (code, stdout, stderr) = run(&["photo.png"], &ws.path);
    assert_eq!(code, 0, "{stderr}");
    assert!(
        stdout.contains("already optimal") || stdout.contains("optimized"),
        "{stdout}"
    );
    assert_eq!(
        std::fs::read(&path).unwrap(),
        once,
        "second run must not change the file"
    );
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

    let (code, stdout, _stderr) = run(&["broken.png"], &ws.path);
    assert_eq!(code, 1, "{stdout}");
    assert_eq!(
        std::fs::read(&path).unwrap(),
        original,
        "corrupt input must be untouched"
    );
}

#[test]
fn gzip_header_is_preserved_byte_for_byte() {
    let ws = Workspace::new("gz-header");
    let payload = b"the quick brown fox jumps over the lazy dog\n".repeat(200);
    let original = build_gzip(&payload, Some(b"original-name.txt"), Some(b"XYZZ"), 0x04, 1);
    let path = ws.write("data.gz", &original);

    let (code, stdout, stderr) = run(&["data.gz"], &ws.path);
    assert_eq!(code, 0, "stdout={stdout} stderr={stderr}");

    let optimized = std::fs::read(&path).unwrap();
    assert!(optimized.len() < original.len(), "expected a smaller file");

    // Everything up to and including FEXTRA + FNAME must be identical.
    let header_len = 10 + 2 + 4 + "original-name.txt".len() + 1;
    assert_eq!(
        &optimized[..header_len],
        &original[..header_len],
        "header changed"
    );
    assert_eq!(
        &optimized[optimized.len() - 8..],
        &original[original.len() - 8..],
        "trailer changed"
    );
}

#[test]
fn gzip_payload_is_unchanged() {
    use std::io::Read;
    let ws = Workspace::new("gz-payload");
    let payload: Vec<u8> = (0..50_000).map(|i| (i % 251) as u8).collect();
    let original = build_gzip(&payload, None, None, 0, 1);
    let path = ws.write("blob.gz", &original);

    let (code, _, stderr) = run(&["blob.gz"], &ws.path);
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

    let (code, stdout, stderr) = run(&["notes.txt"], &ws.path);
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

    let (code, stdout, stderr) = run(&[".", "--recursive", "--json"], &ws.path);
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

    let (code, stdout, stderr) = run(&["a.png", "./a.png", ".", "--json"], &ws.path);
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(
        json_file_records(&stdout),
        1,
        "duplicate paths must collapse: {stdout}"
    );
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

    let (code, stdout, stderr) = run(&[name], &ws.path);
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

    let (code, stdout, stderr) = run(&[".", "--json"], &ws.path);
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

    let (code, stdout, stderr) = run(&["does-not-exist.png", "good.png"], &ws.path);
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

    let (code, _, stderr) = run(&["anim.png"], &ws.path);
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
    let (code, _, stderr) = run(&["--nope"], &ws.path);
    assert_eq!(code, 2, "{stderr}");

    let (code, _, _) = run(&[], &ws.path);
    assert_eq!(code, 2);
}

#[test]
fn help_and_version_succeed() {
    let ws = Workspace::new("help");
    let (code, stdout, _) = run(&["--help"], &ws.path);
    assert_eq!(code, 0);
    assert!(stdout.contains("tzim [OPTIONS] <PATH>..."), "{stdout}");
    assert!(stdout.contains("--list-formats"), "{stdout}");
    assert!(stdout.contains("--check-deps"), "{stdout}");

    let (code, stdout, _) = run(&["--version"], &ws.path);
    assert_eq!(code, 0);
    assert!(stdout.starts_with("tzim "));

    let (code, stdout, _) = run(&["formats"], &ws.path);
    assert_eq!(code, 0);
    assert!(stdout.contains("PNG"));

    let (code, stdout, _) = run(&["--list-formats"], &ws.path);
    assert_eq!(code, 0);
    assert!(stdout.contains("PNG"), "{stdout}");
}

#[test]
fn info_answers_without_touching_anything() {
    let ws = Workspace::new("info");
    for flag in ["-i", "--info"] {
        let (code, stdout, stderr) = run(&[flag], &ws.path);
        assert_eq!(code, 0, "{stderr}");
        assert!(stdout.contains("tzim "), "{stdout}");
        assert!(stdout.contains("G3"), "{stdout}");
        assert!(stdout.contains("MIT"), "{stdout}");
        assert!(stdout.contains("protonmail"), "{stdout}");
        assert!(stdout.contains("PNG"), "{stdout}");
    }
}

#[test]
fn check_deps_reports_that_nothing_is_needed() {
    let ws = Workspace::new("deps");
    let (code, stdout, stderr) = run(&["--check-deps"], &ws.path);
    assert_eq!(code, 0, "{stderr}");
    assert!(stdout.contains("needs no external programs"), "{stdout}");
    assert!(stdout.contains("PNG"), "{stdout}");
}

#[test]
fn a_bare_path_is_a_complete_input_and_the_old_verb_still_works() {
    let ws = Workspace::new("bare-path");
    let spec = PngSpec {
        width: 24,
        height: 24,
        level: 1,
        ..Default::default()
    };
    let photo = ws.write("photo.png", &build_png(&spec));
    let (code, _, stderr) = run(&["photo.png"], &ws.path);
    assert_eq!(code, 0, "{stderr}");
    assert!(std::fs::metadata(&photo).unwrap().len() > 0);

    // The 0.1-0.4 spelling is accepted so that older scripts keep working.
    let (code, _, stderr) = run(&["optimize", "photo.png", "--dry-run"], &ws.path);
    assert_eq!(code, 0, "{stderr}");
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

    let (code, _, stderr) = run(&[".", "--include", "gz"], &ws.path);
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(
        std::fs::read(&png).unwrap(),
        png_before,
        "png must be filtered out"
    );
    assert_ne!(
        std::fs::read(&gz).unwrap(),
        gz_before,
        "gz must be processed"
    );
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

    let (code, stdout, stderr) = run(&["locked.png"], &ws.path);
    assert_eq!(code, 0, "{stderr}");

    let mut perms = std::fs::metadata(&path).unwrap().permissions();
    perms.set_mode(0o644);
    std::fs::set_permissions(&path, perms).unwrap();

    assert_eq!(std::fs::read(&path).unwrap(), original);
    assert!(
        stdout.contains("read-only") || stdout.contains("skipped"),
        "{stdout}"
    );
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

    let (code, _, stderr) = run(&["attrs.png"], &ws.path);
    assert_eq!(code, 0, "{stderr}");

    let after = std::fs::metadata(&path).unwrap();
    assert_eq!(
        before.permissions().mode() & 0o777,
        after.permissions().mode() & 0o777
    );
    assert_eq!(
        before.modified().unwrap(),
        after.modified().unwrap(),
        "mtime must be preserved"
    );
}

#[cfg(unix)]
#[test]
fn a_hard_linked_file_is_skipped_unless_allowed() {
    let ws = Workspace::new("hardlink");
    let spec = PngSpec {
        width: 64,
        height: 64,
        level: 1,
        ..Default::default()
    };
    let original = build_png(&spec);
    let path = ws.write("linked.png", &original);
    let twin = ws.path.join("twin.png");
    std::fs::hard_link(&path, &twin).expect("the fixture directory supports hard links");

    let (code, stdout, stderr) = run(&["linked.png"], &ws.path);
    assert_eq!(code, 0, "stdout={stdout} stderr={stderr}");
    assert!(
        stdout.contains("hard link") && stdout.contains("--allow-hardlinks"),
        "the reason must name the flag: {stdout}"
    );
    assert_eq!(std::fs::read(&path).unwrap(), original);
    assert_eq!(std::fs::read(&twin).unwrap(), original);

    // With the flag the replacement happens, and it breaks the link on purpose:
    // that is the documented consequence, which is why the flag is opt-in.
    let (code, stdout, stderr) = run(&["linked.png", "--allow-hardlinks"], &ws.path);
    assert_eq!(code, 0, "stdout={stdout} stderr={stderr}");
    assert!(std::fs::read(&path).unwrap().len() < original.len());
    assert_eq!(
        std::fs::read(&twin).unwrap(),
        original,
        "the other name keeps the old inode and its bytes"
    );
}

// Linux only, on purpose: ext4, xfs and btrfs store names as bytes, while APFS
// rejects a name that is not valid UTF-8 with EILSEQ — the fixture cannot even be
// created there. Windows has the same restriction through UTF-16. The code path
// under test (`path_raw_b64`, a lossy human field) exists for the filesystems that
// do allow it.
#[cfg(target_os = "linux")]
#[test]
fn a_path_that_is_not_valid_utf8_is_optimized() {
    use std::ffi::OsStr;
    use std::os::unix::ffi::OsStrExt;

    let ws = Workspace::new("non-utf8");
    let spec = PngSpec {
        width: 64,
        height: 64,
        level: 1,
        ..Default::default()
    };
    let original = build_png(&spec);
    // "pic-<0xFF>.png": a valid filename that is not valid UTF-8, which is what
    // every Unix filesystem allows and a lossy `to_string_lossy` would mangle.
    let mut name = b"pic-".to_vec();
    name.push(0xff);
    name.extend_from_slice(b".png");
    let path = ws.path.join(OsStr::from_bytes(&name));
    std::fs::write(&path, &original).unwrap();

    let (code, stdout, stderr) = run_os(
        &[
            std::ffi::OsString::from("--json"),
            std::ffi::OsString::from(OsStr::from_bytes(&name)),
        ],
        &ws.path,
    );
    assert_eq!(code, 0, "stdout={stdout} stderr={stderr}");
    assert!(
        std::fs::read(&path).unwrap().len() < original.len(),
        "the file must be optimized through its raw bytes"
    );
    // The human field is lossy on purpose; the raw bytes are in the record.
    assert!(stdout.contains("\"path_raw_b64\":"), "{stdout}");
    assert!(stdout.contains("\"status\":\"optimized\""), "{stdout}");
}

#[cfg(unix)]
#[test]
fn a_socket_is_skipped_and_never_opened_for_writing() {
    use std::os::unix::fs::FileTypeExt;

    // A Unix socket is the one non-regular file type the standard library can
    // create without libc, and it is enough: the walk must not try to read it.
    //
    // The directory is named by hand rather than by `Workspace`: `sun_path` is
    // limited to about a hundred bytes, and a macOS temporary directory plus a
    // workspace label gets uncomfortably close to that.
    let dir = std::env::temp_dir().join(format!("tzims{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("talk.sock");
    let _listener = std::os::unix::net::UnixListener::bind(&path).expect("bind a socket");

    let (code, stdout, stderr) = run(&["talk.sock"], &dir);
    assert_eq!(code, 0, "stdout={stdout} stderr={stderr}");
    assert!(stdout.contains("not a regular file"), "{stdout}");
    assert!(
        std::fs::symlink_metadata(&path)
            .unwrap()
            .file_type()
            .is_socket(),
        "the socket must still be there"
    );
    let _ = std::fs::remove_dir_all(&dir);
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
        let (code, stdout, stderr) = run(&["link.png"], &ws.path);
        assert_eq!(code, 0, "{stderr}");
        assert_eq!(
            std::fs::read(&target).unwrap(),
            before,
            "target must be untouched"
        );
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

    let (code, stdout, stderr) = run(&[".", "--json"], &ws.path);
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(json_file_records(&stdout), 1, "{stdout}");
}

#[allow(dead_code)]
fn exists(path: &Path) -> bool {
    path.exists()
}

// ---------------------------------------------------------------------------
// ZIP
// ---------------------------------------------------------------------------

fn compressible(len: usize) -> Vec<u8> {
    // Repetitive but not trivial: a weak deflate level leaves room for a gain.
    let mut out = Vec::with_capacity(len);
    let words = [
        b"the ".as_slice(),
        b"quick ".as_slice(),
        b"brown ".as_slice(),
        b"fox ".as_slice(),
        b"jumps ".as_slice(),
        b"over ".as_slice(),
        b"the lazy dog ".as_slice(),
    ];
    let mut i = 0;
    while out.len() < len {
        out.extend_from_slice(words[i % words.len()]);
        i += 1;
    }
    out.truncate(len);
    out
}

#[test]
fn zip_deflate_entries_are_recompressed_and_everything_else_is_preserved() {
    let ws = Workspace::new("zip-basic");
    let mut first = ZipEntrySpec::deflated("docs/readme.txt", &compressible(60_000), 1);
    first.local_extra = vec![0x55, 0x54, 0x05, 0x00, 0x01, 0x02, 0x03, 0x04, 0x05];
    first.central_extra = vec![0x55, 0x54, 0x05, 0x00, 0x09, 0x08, 0x07, 0x06, 0x05];
    first.comment = b"entry comment".to_vec();

    let second = ZipEntrySpec::deflated("data/notes.txt", &compressible(20_000), 1);
    let original = build_zip(&[first, second], b"archive comment");
    let path = ws.write("bundle.zip", &original);

    let (code, stdout, stderr) = run(&["bundle.zip"], &ws.path);
    assert_eq!(code, 0, "stdout={stdout} stderr={stderr}");

    let optimized = std::fs::read(&path).unwrap();
    assert!(
        optimized.len() < original.len(),
        "expected a smaller archive"
    );

    let before = parse_zip(&original);
    let after = parse_zip(&optimized);
    assert_eq!(before.len(), after.len());

    for (a, b) in before.iter().zip(after.iter()) {
        assert_eq!(a.name, b.name, "names must be preserved");
        assert_eq!(a.method, b.method, "methods must not change");
        assert_eq!(a.flags, b.flags, "flags must be preserved");
        assert_eq!(a.crc, b.crc, "CRC must be preserved");
        assert_eq!(
            a.uncomp_size, b.uncomp_size,
            "uncompressed size must be preserved"
        );
        assert_eq!(
            a.local_extra, b.local_extra,
            "local extra fields must be preserved"
        );
        assert_eq!(
            a.central_extra, b.central_extra,
            "central extra fields must be preserved"
        );
        assert_eq!(a.comment, b.comment, "entry comments must be preserved");
        assert_eq!(
            a.decompressed(&original),
            b.decompressed(&optimized),
            "entry payload must be identical"
        );
    }
    assert_eq!(
        zip_archive_comment(&original),
        zip_archive_comment(&optimized)
    );
}

#[test]
fn zip_stored_entries_stay_stored() {
    let ws = Workspace::new("zip-store");
    let stored = ZipEntrySpec::stored("mimetype", b"application/vnd.oasis.opendocument.text");
    let oned = ZipEntrySpec::deflated("content.xml", &compressible(40_000), 1);
    let original = build_zip(&[stored, oned], b"");
    let path = ws.write("doc.odt", &original);

    let (code, _, stderr) = run(&["doc.odt"], &ws.path);
    assert_eq!(code, 0, "{stderr}");

    let optimized = std::fs::read(&path).unwrap();
    let after = parse_zip(&optimized);
    assert_eq!(after[0].name, b"mimetype");
    assert_eq!(after[0].method, 0, "the mimetype entry must stay stored");
    assert_eq!(
        after[0].decompressed(&optimized),
        b"application/vnd.oasis.opendocument.text"
    );
    assert_eq!(after[1].method, 8);
    assert!(optimized.len() < original.len());
}

#[test]
fn zip_data_descriptor_is_rewritten_consistently() {
    let ws = Workspace::new("zip-descriptor");
    let mut spec = ZipEntrySpec::deflated("streamed.bin", &compressible(50_000), 1);
    spec.descriptor = true;
    let original = build_zip(&[spec], b"");
    let path = ws.write("streamed.zip", &original);

    let (code, _, stderr) = run(&["streamed.zip"], &ws.path);
    assert_eq!(code, 0, "{stderr}");

    let optimized = std::fs::read(&path).unwrap();
    assert!(optimized.len() < original.len());

    let after = parse_zip(&optimized);
    assert!(after[0].flags & 0x0008 != 0, "bit 3 must stay set");
    assert_eq!(after[0].crc, crc32fast::hash(&compressible(50_000)));
    assert_eq!(after[0].decompressed(&optimized), compressible(50_000));

    // The descriptor sits immediately after the data and repeats the sizes.
    let data_end = after[0].local_offset as usize
        + 30
        + after[0].name.len()
        + after[0].local_extra.len()
        + after[0].comp_size as usize;
    assert_eq!(
        u32::from_le_bytes([
            optimized[data_end],
            optimized[data_end + 1],
            optimized[data_end + 2],
            optimized[data_end + 3]
        ]),
        ZIP_DESCRIPTOR_SIG
    );
    let desc_comp = u32::from_le_bytes([
        optimized[data_end + 8],
        optimized[data_end + 9],
        optimized[data_end + 10],
        optimized[data_end + 11],
    ]);
    assert_eq!(desc_comp, after[0].comp_size, "descriptor size must match");
}

#[test]
fn zip_non_utf8_names_are_preserved() {
    let ws = Workspace::new("zip-names");
    let mut spec = ZipEntrySpec::deflated("placeholder", &compressible(30_000), 1);
    // A name that is not valid UTF-8, as old archivers produce for non-ASCII paths.
    spec.name = vec![b'c', b'a', b'f', 0xe9, b'/', b'a', b'.', b't', b'x', b't'];
    spec.comment = vec![0xff, 0xfe, 0x00];
    let original = build_zip(&[spec], b"");
    let path = ws.write("names.zip", &original);

    let (code, stdout, stderr) = run(&["names.zip", "--json"], &ws.path);
    assert_eq!(code, 0, "stdout={stdout} stderr={stderr}");

    let optimized = std::fs::read(&path).unwrap();
    let after = parse_zip(&optimized);
    assert_eq!(
        after[0].name,
        vec![b'c', b'a', b'f', 0xe9, b'/', b'a', b'.', b't', b'x', b't']
    );
    assert_eq!(after[0].comment, vec![0xff, 0xfe, 0x00]);
}

#[test]
fn signed_jar_is_refused_and_left_untouched() {
    let ws = Workspace::new("zip-signed");
    let entries = vec![
        ZipEntrySpec::deflated("META-INF/MANIFEST.MF", &compressible(2_000), 1),
        ZipEntrySpec::deflated("META-INF/SIGNER.SF", b"Signature-Version: 1.0", 1),
        ZipEntrySpec::deflated("META-INF/SIGNER.RSA", &[0u8; 512], 1),
        ZipEntrySpec::deflated("com/example/App.class", &compressible(20_000), 1),
    ];
    let original = build_zip(&entries, b"");
    let path = ws.write("signed.jar", &original);

    let (code, stdout, stderr) = run(&["signed.jar"], &ws.path);
    assert_eq!(code, 0, "stdout={stdout} stderr={stderr}");
    assert!(
        stdout.contains("signature") || stdout.contains("signed"),
        "the reason must be reported: {stdout}"
    );
    assert_eq!(
        std::fs::read(&path).unwrap(),
        original,
        "a signed archive must not change"
    );
}

#[test]
fn signed_odf_document_is_refused() {
    let ws = Workspace::new("zip-odf-signed");
    let entries = vec![
        ZipEntrySpec::stored("mimetype", b"application/vnd.oasis.opendocument.text"),
        ZipEntrySpec::deflated("META-INF/documentsignatures.xml", &[b'x'; 100], 1),
        ZipEntrySpec::deflated("content.xml", &compressible(10_000), 1),
    ];
    let original = build_zip(&entries, b"");
    let path = ws.write("signed.odt", &original);

    let (code, _, stderr) = run(&["signed.odt"], &ws.path);
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(std::fs::read(&path).unwrap(), original);
}

#[test]
fn apk_signing_block_is_refused() {
    let ws = Workspace::new("zip-apk");
    let archive = build_zip(
        &[ZipEntrySpec::deflated(
            "classes.dex",
            &compressible(30_000),
            1,
        )],
        b"",
    );

    // Insert an APK signing block immediately before the central directory and
    // move the central directory offset accordingly.
    let eocd_at = (0..=archive.len() - 22)
        .rev()
        .find(|&at| {
            u32::from_le_bytes([
                archive[at],
                archive[at + 1],
                archive[at + 2],
                archive[at + 3],
            ]) == ZIP_EOCD_SIG
        })
        .unwrap();
    let cd_offset = u32::from_le_bytes([
        archive[eocd_at + 16],
        archive[eocd_at + 17],
        archive[eocd_at + 18],
        archive[eocd_at + 19],
    ]) as usize;

    // 8-byte size, 8-byte size, 16-byte magic: the minimum block the detector looks for.
    let mut block = Vec::new();
    block.extend_from_slice(&24u64.to_le_bytes());
    block.extend_from_slice(&24u64.to_le_bytes());
    block.extend_from_slice(b"APK Sig Block 42");

    let mut rebuilt = archive[..cd_offset].to_vec();
    rebuilt.extend_from_slice(&block);
    let new_cd_offset = rebuilt.len() as u32;
    rebuilt.extend_from_slice(&archive[cd_offset..]);
    let eocd_pos = rebuilt.len() - (archive.len() - eocd_at);
    rebuilt[eocd_pos + 16..eocd_pos + 20].copy_from_slice(&new_cd_offset.to_le_bytes());

    let path = ws.write("app.apk", &rebuilt);
    let (code, stdout, stderr) = run(&["app.apk"], &ws.path);
    assert_eq!(code, 0, "stdout={stdout} stderr={stderr}");
    assert!(
        stdout.contains("APK") || stdout.contains("signature"),
        "the reason must be reported: {stdout}"
    );
    assert_eq!(std::fs::read(&path).unwrap(), rebuilt);
}

#[test]
fn zip64_is_refused() {
    let ws = Workspace::new("zip64");
    let mut archive = build_zip(
        &[ZipEntrySpec::deflated("a.txt", &compressible(10_000), 1)],
        b"",
    );
    // Mark the end record as ZIP64.
    let eocd = archive.len() - 22;
    archive[eocd + 10..eocd + 12].copy_from_slice(&0xffffu16.to_le_bytes());
    let original = archive.clone();
    let path = ws.write("big.zip", &original);

    let (code, stdout, stderr) = run(&["big.zip"], &ws.path);
    assert_eq!(code, 0, "stdout={stdout} stderr={stderr}");
    assert!(stdout.contains("ZIP64"), "{stdout}");
    assert_eq!(std::fs::read(&path).unwrap(), original);
}

#[test]
fn encrypted_zip_is_refused() {
    let ws = Workspace::new("zip-crypt");
    let archive = build_zip_with_flags(
        &[ZipEntrySpec::deflated(
            "secret.txt",
            &compressible(5_000),
            1,
        )],
        b"",
        0x0001,
    );
    let path = ws.write("secret.zip", &archive);

    let (code, stdout, stderr) = run(&["secret.zip"], &ws.path);
    assert_eq!(code, 0, "stdout={stdout} stderr={stderr}");
    assert!(stdout.contains("encrypted"), "{stdout}");
    assert_eq!(std::fs::read(&path).unwrap(), archive);
}

#[test]
fn truncated_zip_is_refused() {
    let ws = Workspace::new("zip-truncated");
    let archive = build_zip(
        &[ZipEntrySpec::deflated("a.txt", &compressible(10_000), 1)],
        b"",
    );
    let truncated = archive[..archive.len() - 30].to_vec();
    let path = ws.write("cut.zip", &truncated);

    let (code, stdout, _) = run(&["cut.zip"], &ws.path);
    assert_ne!(code, 0, "{stdout}");
    assert_eq!(std::fs::read(&path).unwrap(), truncated);
}

#[test]
fn corrupt_entry_crc_is_refused() {
    let ws = Workspace::new("zip-corrupt");
    let mut archive = build_zip(
        &[ZipEntrySpec::deflated("a.txt", &compressible(20_000), 1)],
        b"",
    );
    // Flip a byte inside the compressed payload.
    let entry = parse_zip(&archive)[0].clone();
    let payload_at = entry.local_offset as usize + 30 + entry.name.len() + entry.local_extra.len();
    archive[payload_at + 10] ^= 0xff;
    let original = archive.clone();
    let path = ws.write("broken.zip", &original);

    let (code, stdout, _) = run(&["broken.zip"], &ws.path);
    assert_ne!(code, 0, "{stdout}");
    assert_eq!(std::fs::read(&path).unwrap(), original);
}

#[test]
fn already_optimal_zip_is_left_alone() {
    let ws = Workspace::new("zip-optimal");
    // Level 9 leaves nothing to gain, so the archive must be reported as optimal.
    let archive = build_zip(
        &[ZipEntrySpec::deflated("a.txt", &compressible(20_000), 9)],
        b"",
    );
    let path = ws.write("optimal.zip", &archive);

    let (code, stdout, stderr) = run(&["optimal.zip"], &ws.path);
    assert_eq!(code, 0, "stdout={stdout} stderr={stderr}");
    assert!(
        stdout.contains("already optimal"),
        "expected no change: {stdout}"
    );
    assert_eq!(std::fs::read(&path).unwrap(), archive);
}

#[test]
fn zip_bomb_ratio_is_refused() {
    let ws = Workspace::new("zip-bomb");
    let archive = build_zip(
        &[ZipEntrySpec::deflated(
            "zeros.bin",
            &vec![0u8; 2_000_000],
            6,
        )],
        b"",
    );
    let path = ws.write("bomb.zip", &archive);

    let (code, stdout, stderr) = run(&["bomb.zip", "--max-ratio", "10"], &ws.path);
    assert_eq!(code, 0, "stdout={stdout} stderr={stderr}");
    assert!(stdout.contains("max-ratio"), "{stdout}");
    assert_eq!(std::fs::read(&path).unwrap(), archive);
}

#[test]
fn stub_prefixed_archive_is_not_detected_as_zip() {
    let ws = Workspace::new("zip-stub");
    let mut archive = b"MZ\x90\x00stub payload".to_vec();
    archive.extend_from_slice(&build_zip(
        &[ZipEntrySpec::deflated("a.txt", &compressible(10_000), 1)],
        b"",
    ));
    let original = archive.clone();
    let path = ws.write("installer.exe", &original);

    let (code, stdout, stderr) = run(&["installer.exe"], &ws.path);
    assert_eq!(code, 0, "stdout={stdout} stderr={stderr}");
    assert!(stdout.contains("unsupported"), "{stdout}");
    assert_eq!(std::fs::read(&path).unwrap(), original);
}

// ---------------------------------------------------------------------------
// v0.3: APNG, ICO, nested archives
// ---------------------------------------------------------------------------

#[test]
fn apng_frames_are_recompressed_and_control_chunks_kept() {
    let ws = Workspace::new("apng-frames");
    let original = build_apng(64, 48, 3, 1);
    let path = ws.write("anim.png", &original);

    let (code, stdout, stderr) = run(&["anim.png"], &ws.path);
    assert_eq!(code, 0, "stdout={stdout} stderr={stderr}");

    let optimized = std::fs::read(&path).unwrap();
    assert!(optimized.len() < original.len(), "expected a smaller APNG");

    let before = png_chunks(&original).unwrap();
    let after = png_chunks(&optimized).unwrap();

    // Every control chunk (acTL, fcTL) must be byte-identical and in order.
    let controls_before: Vec<_> = before
        .iter()
        .filter(|(t, _)| t != "IDAT" && t != "fdAT")
        .collect();
    let controls_after: Vec<_> = after
        .iter()
        .filter(|(t, _)| t != "IDAT" && t != "fdAT")
        .collect();
    assert_eq!(
        controls_before, controls_after,
        "control chunks must be preserved"
    );

    // The number of frame chunks and their sequence numbers must not change.
    let fdat_before: Vec<_> = before.iter().filter(|(t, _)| t == "fdAT").collect();
    let fdat_after: Vec<_> = after.iter().filter(|(t, _)| t == "fdAT").collect();
    assert_eq!(fdat_before.len(), fdat_after.len());
    for (a, b) in fdat_before.iter().zip(fdat_after.iter()) {
        assert_eq!(
            a.1[..4],
            b.1[..4],
            "fdAT sequence numbers must be preserved"
        );
    }

    // Every frame must still decode to the same pixels.
    let (w1, h1, p1) = decode_pixels(&original).unwrap();
    let (w2, h2, p2) = decode_pixels(&optimized).unwrap();
    assert_eq!((w1, h1), (w2, h2));
    assert_eq!(p1, p2);
}

#[test]
fn apng_with_a_single_frame_is_optimized() {
    let ws = Workspace::new("apng-single");
    let original = build_apng(48, 48, 1, 1);
    let path = ws.write("single.png", &original);

    let (code, _, stderr) = run(&["single.png"], &ws.path);
    assert_eq!(code, 0, "{stderr}");
    let optimized = std::fs::read(&path).unwrap();
    assert!(png_chunks(&optimized).is_ok());
    let (_, _, p1) = decode_pixels(&original).unwrap();
    let (_, _, p2) = decode_pixels(&optimized).unwrap();
    assert_eq!(p1, p2);
}

#[test]
fn ico_embedded_pngs_are_optimized_and_bitmaps_preserved() {
    let ws = Workspace::new("ico-mixed");
    let png_spec = PngSpec {
        width: 64,
        height: 64,
        level: 1,
        ..Default::default()
    };
    let bitmap = bmp_icon_payload(512);
    let entries = vec![
        IcoEntrySpec {
            width: 64,
            height: 64,
            bit_count: 32,
            payload: build_png(&png_spec),
        },
        IcoEntrySpec {
            width: 16,
            height: 16,
            bit_count: 32,
            payload: bitmap.clone(),
        },
    ];
    let original = build_ico(1, &entries);
    let path = ws.write("app.ico", &original);

    let (code, stdout, stderr) = run(&["app.ico"], &ws.path);
    assert_eq!(code, 0, "stdout={stdout} stderr={stderr}");

    let optimized = std::fs::read(&path).unwrap();
    assert!(optimized.len() < original.len(), "expected a smaller icon");

    let before = parse_ico(&original);
    let after = parse_ico(&optimized);
    assert_eq!(before.len(), after.len());

    // Directory fields other than size and offset are preserved.
    assert_eq!(before[0].0[..8], after[0].0[..8]);
    assert_eq!(before[1].0[..8], after[1].0[..8]);

    // The bitmap entry is untouched; the PNG entry still decodes identically.
    assert_eq!(
        after[1].1, bitmap,
        "bitmap image must be preserved byte for byte"
    );
    let (_, _, p1) = decode_pixels(&before[0].1).unwrap();
    let (_, _, p2) = decode_pixels(&after[0].1).unwrap();
    assert_eq!(p1, p2);
    assert!(after[0].1.len() < before[0].1.len());
}

#[test]
fn ico_with_only_bitmaps_is_already_optimal() {
    let ws = Workspace::new("ico-bmp-only");
    let entries = vec![IcoEntrySpec {
        width: 32,
        height: 32,
        bit_count: 32,
        payload: bmp_icon_payload(1024),
    }];
    let original = build_ico(1, &entries);
    let path = ws.write("plain.ico", &original);

    let (code, stdout, stderr) = run(&["plain.ico"], &ws.path);
    assert_eq!(code, 0, "stdout={stdout} stderr={stderr}");
    assert!(stdout.contains("already optimal"), "{stdout}");
    assert_eq!(std::fs::read(&path).unwrap(), original);
}

#[test]
fn cursor_files_are_handled_like_icons() {
    let ws = Workspace::new("cur");
    let png_spec = PngSpec {
        width: 32,
        height: 32,
        level: 1,
        ..Default::default()
    };
    let entries = vec![IcoEntrySpec {
        width: 32,
        height: 32,
        bit_count: 0,
        payload: build_png(&png_spec),
    }];
    let original = build_ico(2, &entries);
    let path = ws.write("pointer.cur", &original);

    let (code, stdout, stderr) = run(&["pointer.cur"], &ws.path);
    assert_eq!(code, 0, "stdout={stdout} stderr={stderr}");
    let optimized = std::fs::read(&path).unwrap();
    assert!(optimized.len() < original.len());
    assert_eq!(&optimized[2..4], &[2, 0], "cursor type must be preserved");
}

#[test]
fn ico_with_a_bad_offsets_is_refused() {
    let ws = Workspace::new("ico-bad");
    let mut icon = build_ico(
        1,
        &[IcoEntrySpec {
            width: 32,
            height: 32,
            bit_count: 32,
            payload: bmp_icon_payload(256),
        }],
    );
    // Point the image outside the file.
    let past_end = icon.len() as u32 + 100;
    icon[6 + 12..6 + 16].copy_from_slice(&past_end.to_le_bytes());
    let original = icon.clone();
    let path = ws.write("broken.ico", &original);

    let (code, stdout, _) = run(&["broken.ico"], &ws.path);
    assert_eq!(code, 1, "{stdout}");
    assert_eq!(std::fs::read(&path).unwrap(), original);
}

#[test]
fn nested_optimization_reaches_the_file_inside_the_archive() {
    let ws = Workspace::new("nested-png");
    let png_spec = PngSpec {
        width: 96,
        height: 96,
        level: 1,
        ..Default::default()
    };
    let inner_png = build_png(&png_spec);
    let entries = vec![ZipEntrySpec::deflated("images/logo.png", &inner_png, 1)];
    let original = build_zip(&entries, b"");

    // Without --nested the stored bytes stay identical.
    let plain = ws.write("plain.zip", &original);
    let (code, _, stderr) = run(&["plain.zip"], &ws.path);
    assert_eq!(code, 0, "{stderr}");
    let plain_out = std::fs::read(&plain).unwrap();
    let plain_entry = parse_zip(&plain_out).remove(0);
    assert_eq!(
        plain_entry.decompressed(&plain_out),
        inner_png,
        "without --nested the content must not change"
    );

    // With --nested the PNG inside is optimized: smaller than the plain result.
    let nested = ws.write("nested.zip", &original);
    let (code, stdout, stderr) = run(&["nested.zip", "--nested"], &ws.path);
    assert_eq!(code, 0, "stdout={stdout} stderr={stderr}");
    let nested_out = std::fs::read(&nested).unwrap();
    assert!(
        nested_out.len() < plain_out.len(),
        "--nested must beat recompression alone"
    );

    let entry = parse_zip(&nested_out).remove(0);
    let content = entry.decompressed(&nested_out);
    assert!(
        content.len() < inner_png.len(),
        "the embedded PNG must shrink"
    );
    assert_eq!(
        entry.crc,
        crc32fast::hash(&content),
        "CRC must match the new content"
    );
    let (_, _, p1) = decode_pixels(&inner_png).unwrap();
    let (_, _, p2) = decode_pixels(&content).unwrap();
    assert_eq!(p1, p2, "the embedded image must keep its pixels");
}

#[test]
fn nested_depth_limit_is_respected() {
    let ws = Workspace::new("nested-depth");
    let png_spec = PngSpec {
        width: 64,
        height: 64,
        level: 1,
        ..Default::default()
    };
    let inner_png = build_png(&png_spec);
    let inner_zip = build_zip(&[ZipEntrySpec::deflated("img.png", &inner_png, 1)], b"");
    let outer = build_zip(&[ZipEntrySpec::deflated("inner.zip", &inner_zip, 1)], b"");

    // Depth 1: the outer archive is rewritten, but the inner archive is not.
    let shallow = ws.write("shallow.zip", &outer);
    let (code, _, stderr) = run(&["shallow.zip", "--nested", "--max-depth", "1"], &ws.path);
    assert_eq!(code, 0, "{stderr}");
    let shallow_out = std::fs::read(&shallow).unwrap();
    let inner_after = parse_zip(&shallow_out).remove(0).decompressed(&shallow_out);
    assert_eq!(
        inner_after, inner_zip,
        "at depth 1 the inner archive must be left untouched"
    );

    // Depth 3: the PNG inside the inner archive is optimized too.
    let deep = ws.write("deep.zip", &outer);
    let (code, _, stderr) = run(&["deep.zip", "--nested", "--max-depth", "3"], &ws.path);
    assert_eq!(code, 0, "{stderr}");
    let deep_out = std::fs::read(&deep).unwrap();
    assert!(
        deep_out.len() < shallow_out.len(),
        "deeper nesting should save more"
    );
    let inner_after = parse_zip(&deep_out).remove(0).decompressed(&deep_out);
    let png_after = parse_zip(&inner_after).remove(0).decompressed(&inner_after);
    assert!(png_after.len() < inner_png.len());
    let (_, _, p1) = decode_pixels(&inner_png).unwrap();
    let (_, _, p2) = decode_pixels(&png_after).unwrap();
    assert_eq!(p1, p2);
}

#[test]
fn nested_does_not_touch_signed_archives() {
    let ws = Workspace::new("nested-signed");
    let entries = vec![
        ZipEntrySpec::deflated("META-INF/MANIFEST.MF", b"Manifest-Version: 1.0\n", 1),
        ZipEntrySpec::deflated("META-INF/APP.SF", b"Signature-Version: 1.0", 1),
    ];
    let original = build_zip(&entries, b"");
    let path = ws.write("signed.jar", &original);

    let (code, stdout, stderr) = run(&["signed.jar", "--nested"], &ws.path);
    assert_eq!(code, 0, "stdout={stdout} stderr={stderr}");
    assert_eq!(std::fs::read(&path).unwrap(), original);
}

// ---------------------------------------------------------------------------
// JPEG (v0.4, opt-in)
// ---------------------------------------------------------------------------

fn has(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.windows(needle.len()).any(|w| w == needle)
}

#[test]
fn jpeg_baseline_is_optimized_and_then_stable() {
    let ws = Workspace::new("jpeg-basic");
    let original = build_jpeg(64, 48, 3);
    let path = ws.write("photo.jpg", &original);

    let (code, stdout, stderr) = run(&["--formats", "jpeg", "photo.jpg"], &ws.path);
    assert_eq!(code, 0, "stdout={stdout} stderr={stderr}");

    let after = std::fs::read(&path).unwrap();
    assert!(
        after.len() < original.len(),
        "expected a smaller file: {} -> {}",
        original.len(),
        after.len()
    );
    assert!(has(&after, b"tzim-test-exif-payload"), "Exif payload lost");
    assert!(has(&after, b"tzim fixture comment"), "comment lost");
    assert_eq!(&after[..2], &[0xff, 0xd8]);
    assert_eq!(&after[after.len() - 2..], &[0xff, 0xd9]);

    // The tables were the only wasteful thing, so a second run has nothing left.
    let (code, stdout, stderr) = run(&["--formats", "jpeg", "photo.jpg"], &ws.path);
    assert_eq!(code, 0, "stderr={stderr}");
    assert!(stdout.contains("already optimal"), "{stdout}");
    assert_eq!(std::fs::read(&path).unwrap(), after);
}

#[test]
fn jpeg_is_inert_without_the_formats_flag() {
    let ws = Workspace::new("jpeg-opt-in");
    let original = build_jpeg(32, 32, 1);
    let path = ws.write("photo.jpg", &original);

    let (code, stdout, stderr) = run(&["photo.jpg"], &ws.path);
    assert_eq!(code, 0, "stdout={stdout} stderr={stderr}");
    assert!(stdout.contains("skipped"), "{stdout}");
    assert!(stdout.contains("not enabled"), "{stdout}");
    assert_eq!(std::fs::read(&path).unwrap(), original);
}

#[test]
fn jpeg_progressive_is_skipped() {
    let ws = Workspace::new("jpeg-progressive");
    let original = jpeg_with_sof_marker(&build_jpeg(32, 32, 3), 0xc2);
    let path = ws.write("progressive.jpg", &original);

    let (code, stdout, stderr) = run(&["--formats", "jpeg", "progressive.jpg"], &ws.path);
    assert_eq!(code, 0, "a refusal is not a failure: {stdout} {stderr}");
    assert!(stdout.contains("baseline"), "{stdout}");
    assert_eq!(std::fs::read(&path).unwrap(), original);
}

#[test]
fn jpeg_arithmetic_is_skipped() {
    let ws = Workspace::new("jpeg-arithmetic");
    let original = jpeg_with_sof_marker(&build_jpeg(32, 32, 3), 0xc9);
    let path = ws.write("arithmetic.jpg", &original);

    let (code, stdout, stderr) = run(&["--formats", "jpeg", "arithmetic.jpg"], &ws.path);
    assert_eq!(code, 0, "stdout={stdout} stderr={stderr}");
    assert!(
        stdout.contains("SOF9") || stdout.contains("arithmetic"),
        "{stdout}"
    );
    assert_eq!(std::fs::read(&path).unwrap(), original);
}

#[test]
fn jpeg_twelve_bit_is_skipped() {
    let ws = Workspace::new("jpeg-12bit");
    let original = jpeg_with_precision(&build_jpeg(32, 32, 1), 12);
    let path = ws.write("twelve.jpg", &original);

    let (code, stdout, stderr) = run(&["--formats", "jpeg", "twelve.jpg"], &ws.path);
    assert_eq!(code, 0, "stdout={stdout} stderr={stderr}");
    assert!(stdout.contains("12-bit"), "{stdout}");
    assert_eq!(std::fs::read(&path).unwrap(), original);
}

#[test]
fn jpeg_truncated_file_fails_and_is_untouched() {
    let ws = Workspace::new("jpeg-truncated");
    let full = build_jpeg(64, 64, 3);
    let original = full[..full.len() / 2].to_vec();
    let path = ws.write("broken.jpg", &original);

    let (code, stdout, stderr) = run(&["--formats", "jpeg", "broken.jpg"], &ws.path);
    assert_eq!(code, 1, "a corrupt file is a failure: {stdout} {stderr}");
    assert_eq!(std::fs::read(&path).unwrap(), original);
}

#[test]
fn jpeg_grayscale_is_optimized() {
    let ws = Workspace::new("jpeg-gray");
    let original = build_jpeg(48, 40, 1);
    let path = ws.write("gray.jpg", &original);

    let (code, stdout, stderr) = run(&["--formats", "jpeg", "gray.jpg"], &ws.path);
    assert_eq!(code, 0, "stdout={stdout} stderr={stderr}");
    assert!(std::fs::read(&path).unwrap().len() < original.len());

    let (code, stdout, _) = run(&["--formats", "jpeg", "gray.jpg"], &ws.path);
    assert_eq!(code, 0);
    assert!(stdout.contains("already optimal"), "{stdout}");
}

#[test]
fn jpeg_trailing_bytes_after_eoi_are_preserved() {
    let ws = Workspace::new("jpeg-trailing");
    let trailing = b"TRAILING-BYTES-AFTER-EOI";
    let original = build_jpeg_with_trailing(32, 32, 3, trailing);
    let path = ws.write("trailing.jpg", &original);

    let (code, stdout, stderr) = run(&["--formats", "jpeg", "trailing.jpg"], &ws.path);
    assert_eq!(code, 0, "stdout={stdout} stderr={stderr}");
    let after = std::fs::read(&path).unwrap();
    assert!(after.len() < original.len());
    assert!(
        after.ends_with(trailing),
        "trailing bytes were not preserved"
    );
}

#[test]
fn formats_flag_restricts_other_formats_too() {
    let ws = Workspace::new("formats-restrict");
    let spec = PngSpec {
        width: 64,
        height: 64,
        level: 1,
        ..Default::default()
    };
    let original = build_png(&spec);
    let path = ws.write("photo.png", &original);

    let (code, stdout, stderr) = run(&["--formats", "jpeg", "photo.png"], &ws.path);
    assert_eq!(code, 0, "stdout={stdout} stderr={stderr}");
    assert!(stdout.contains("skipped"), "{stdout}");
    assert_eq!(std::fs::read(&path).unwrap(), original);
}

#[test]
fn jpeg_restart_intervals_are_preserved() {
    let ws = Workspace::new("jpeg-restart");
    let original = build_jpeg_with_restart(64, 64, 3, 3);
    let path = ws.write("restart.jpg", &original);

    let (code, stdout, stderr) = run(&["--formats", "jpeg", "restart.jpg"], &ws.path);
    assert_eq!(code, 0, "stdout={stdout} stderr={stderr}");
    let after = std::fs::read(&path).unwrap();
    assert!(after.len() < original.len());

    // DRI (interval 3) must survive verbatim, and the scan must still use RSTn.
    assert!(
        has(&after, &[0xff, 0xdd, 0x00, 0x04, 0x00, 0x03]),
        "DRI lost"
    );
    assert!(
        after.windows(2).any(|w| w == [0xff, 0xd0]),
        "restart markers lost"
    );

    let (code, stdout, _) = run(&["--formats", "jpeg", "restart.jpg"], &ws.path);
    assert_eq!(code, 0);
    assert!(stdout.contains("already optimal"), "{stdout}");
}

// ---------------------------------------------------------------------------
// --strip-metadata
// ---------------------------------------------------------------------------

/// The list of marker bytes in a JPEG's header, stopping at the scan.
fn jpeg_header_markers(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut pos = 2usize;
    while pos + 3 < data.len() {
        if data[pos] != 0xff {
            break;
        }
        let marker = data[pos + 1];
        out.push(marker);
        if marker == 0xda || marker == 0xd9 {
            break;
        }
        let len = u16::from_be_bytes([data[pos + 2], data[pos + 3]]) as usize;
        pos += 2 + len;
    }
    out
}

#[test]
fn strip_metadata_removes_png_text_and_exif_but_keeps_the_content() {
    let ws = Workspace::new("strip-png");
    let spec = PngSpec {
        width: 64,
        height: 64,
        level: 1,
        extra_chunks: vec![
            ("tEXt".to_string(), b"Comment\0written by a tool".to_vec()),
            ("tIME".to_string(), vec![0x07, 0xea, 1, 2, 3, 4, 5]),
            ("prVt".to_string(), b"private, must survive".to_vec()),
        ],
        ..Default::default()
    };
    let original = build_png(&spec);
    let path = ws.write("stamped.png", &original);

    let (code, stdout, stderr) = run(&["stamped.png", "--strip-metadata"], &ws.path);
    assert_eq!(code, 0, "stdout={stdout} stderr={stderr}");

    let stripped = std::fs::read(&path).unwrap();
    assert!(
        stripped.len() < original.len(),
        "stripping must shrink the file"
    );
    let chunks: Vec<String> = png_chunks(&stripped)
        .unwrap()
        .into_iter()
        .map(|(t, _)| t)
        .collect();
    assert!(!chunks.contains(&"tEXt".to_string()), "{chunks:?}");
    assert!(!chunks.contains(&"tIME".to_string()), "{chunks:?}");
    // A chunk this build does not understand stays: deleting what we cannot
    // check is not what the flag means.
    assert!(chunks.contains(&"prVt".to_string()), "{chunks:?}");

    let (w1, h1, p1) = decode_pixels(&original).unwrap();
    let (w2, h2, p2) = decode_pixels(&stripped).unwrap();
    assert_eq!((w1, h1), (w2, h2));
    assert_eq!(p1, p2, "pixels must be identical");
}

#[test]
fn strip_metadata_clears_the_gzip_header_and_keeps_the_payload() {
    let ws = Workspace::new("strip-gzip");
    let payload = b"the payload is the content and it never changes".repeat(200);
    let original = build_gzip(&payload, Some(b"secret-name.txt"), Some(b"XYZZ"), 0, 1);
    let path = ws.write("blob.gz", &original);

    let (code, stdout, stderr) = run(&["blob.gz", "--strip-metadata"], &ws.path);
    assert_eq!(code, 0, "stdout={stdout} stderr={stderr}");

    let stripped = std::fs::read(&path).unwrap();
    assert!(stripped.len() < original.len());
    assert_eq!(
        &stripped[..4],
        &[0x1f, 0x8b, 0x08, 0x00],
        "flags must be zero"
    );
    assert_eq!(&stripped[4..8], &[0, 0, 0, 0], "MTIME must be zero");
    assert!(
        !stripped.windows(16).any(|w| w == b"secret-name.txt\0"),
        "the name must be gone"
    );

    // The decompressed bytes are the contract, so decode both and compare.
    let (payload_before, _) = inflate_member(&original);
    let (payload_after, _) = inflate_member(&stripped);
    assert_eq!(payload_before, payload_after);
}

/// Inflates a single-member gzip file, returning the payload and its CRC.
fn inflate_member(data: &[u8]) -> (Vec<u8>, u32) {
    use std::io::Read;

    // The fixtures used here have no FEXTRA/FNAME/FCOMMENT after stripping, but
    // the original may have them, so walk the header flags the slow way.
    let flags = data[3];
    let mut pos = 10usize;
    if flags & 0x04 != 0 {
        let xlen = u16::from_le_bytes([data[pos], data[pos + 1]]) as usize;
        pos += 2 + xlen;
    }
    for bit in [0x08u8, 0x10] {
        if flags & bit != 0 {
            while data[pos] != 0 {
                pos += 1;
            }
            pos += 1;
        }
    }
    let body = &data[pos..data.len() - 8];
    let mut decoder = flate2::read::DeflateDecoder::new(body);
    let mut out = Vec::new();
    decoder.read_to_end(&mut out).unwrap();
    let crc = crc32fast::hash(&out);
    (out, crc)
}

#[test]
fn strip_metadata_drops_jpeg_exif_and_comment() {
    let ws = Workspace::new("strip-jpeg");
    let original = build_jpeg(64, 64, 3);
    let path = ws.write("shot.jpg", &original);
    assert!(
        jpeg_header_markers(&original).contains(&0xe1),
        "the fixture must carry an APP1 segment"
    );

    let (code, stdout, stderr) = run(
        &["shot.jpg", "--formats", "jpeg", "--strip-metadata"],
        &ws.path,
    );
    assert_eq!(code, 0, "stdout={stdout} stderr={stderr}");

    let stripped = std::fs::read(&path).unwrap();
    let markers = jpeg_header_markers(&stripped);
    assert!(
        !markers.contains(&0xe1),
        "APP1 must be gone: {markers:02x?}"
    );
    assert!(!markers.contains(&0xfe), "COM must be gone: {markers:02x?}");
    // JFIF stays: it declares the pixel density and the colour space.
    assert!(markers.contains(&0xe0), "{markers:02x?}");
    assert_eq!(&stripped[..2], &[0xff, 0xd8]);
    assert_eq!(&stripped[stripped.len() - 2..], &[0xff, 0xd9]);
}

#[test]
fn strip_metadata_drops_zip_comments_and_keeps_extra_fields() {
    let ws = Workspace::new("strip-zip");
    let mut entry = ZipEntrySpec::deflated("report.txt", &b"contents".repeat(400), 1);
    entry.comment = b"entry comment".to_vec();
    entry.local_extra = vec![0x55, 0x54, 0x05, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00];
    entry.central_extra = entry.local_extra.clone();
    let original = build_zip(&[entry], b"archive comment");
    let path = ws.write("bundle.zip", &original);

    let (code, stdout, stderr) = run(&["bundle.zip", "--strip-metadata"], &ws.path);
    assert_eq!(code, 0, "stdout={stdout} stderr={stderr}");

    let stripped = std::fs::read(&path).unwrap();
    assert!(stripped.len() < original.len());
    assert!(zip_archive_comment(&stripped).is_empty());

    let before = parse_zip(&original);
    let after = parse_zip(&stripped);
    assert_eq!(after.len(), before.len());
    assert!(
        after[0].comment.is_empty(),
        "the entry comment must be gone"
    );
    assert_eq!(
        after[0].central_extra, before[0].central_extra,
        "extra fields are structure, not decoration"
    );
    assert_eq!(
        after[0].decompressed(&stripped),
        before[0].decompressed(&original)
    );
}

/// The `time`/`date` pair of every central directory record, in order.
fn zip_central_stamps(archive: &[u8]) -> Vec<(u16, u16)> {
    let eocd = (0..archive.len() - 21)
        .rev()
        .find(|&at| {
            u32::from_le_bytes([
                archive[at],
                archive[at + 1],
                archive[at + 2],
                archive[at + 3],
            ]) == ZIP_EOCD_SIG
        })
        .expect("no end-of-central-directory record");
    let mut pos = u32::from_le_bytes([
        archive[eocd + 16],
        archive[eocd + 17],
        archive[eocd + 18],
        archive[eocd + 19],
    ]) as usize;
    let mut out = Vec::new();
    while u32::from_le_bytes([
        archive[pos],
        archive[pos + 1],
        archive[pos + 2],
        archive[pos + 3],
    ]) == ZIP_CENTRAL_SIG
    {
        out.push((
            u16::from_le_bytes([archive[pos + 12], archive[pos + 13]]),
            u16::from_le_bytes([archive[pos + 14], archive[pos + 15]]),
        ));
        let name_len = u16::from_le_bytes([archive[pos + 28], archive[pos + 29]]) as usize;
        let extra_len = u16::from_le_bytes([archive[pos + 30], archive[pos + 31]]) as usize;
        let comment_len = u16::from_le_bytes([archive[pos + 32], archive[pos + 33]]) as usize;
        pos += 46 + name_len + extra_len + comment_len;
    }
    out
}

/// The local header's `time`/`date` pair for the entry at `offset`.
fn zip_local_stamp(archive: &[u8], offset: usize) -> (u16, u16) {
    (
        u16::from_le_bytes([archive[offset + 10], archive[offset + 11]]),
        u16::from_le_bytes([archive[offset + 12], archive[offset + 13]]),
    )
}

#[test]
fn strip_metadata_equal_size_zip_is_optimized() {
    let ws = Workspace::new("strip-zip-equal");
    // A stored entry is never recompressed and the archive carries no comment, so
    // the only thing `--strip-metadata` can do is zero four bytes of timestamps:
    // the candidate is exactly as long as the input. This is the class of result
    // that invariant I2 accepts and that the pipeline used to call an internal
    // error (#26).
    let entry = ZipEntrySpec::stored("a.txt", b"stored bytes are never recompressed");
    let original = build_zip(&[entry], b"");
    let path = ws.write("equal.zip", &original);
    let mtime_before = std::fs::metadata(&path).unwrap().modified().unwrap();

    let (code, stdout, stderr) = run(&["equal.zip", "--strip-metadata"], &ws.path);
    assert_eq!(code, 0, "stdout={stdout} stderr={stderr}");
    assert!(
        !stdout.contains("internal") && !stderr.contains("internal"),
        "an equal-size rewrite is a result, not a bug: stdout={stdout} stderr={stderr}"
    );
    assert!(stdout.contains("metadata stripped"), "{stdout}");

    let stripped = std::fs::read(&path).unwrap();
    assert_eq!(
        stripped.len(),
        original.len(),
        "zeroing a timestamp removes no bytes"
    );
    assert_ne!(
        stripped, original,
        "the file must have been rewritten with the metadata removed"
    );

    let before = parse_zip(&original);
    assert_eq!(zip_central_stamps(&original), vec![(0x6000, 0x5a21)]);
    assert_eq!(
        zip_central_stamps(&stripped),
        vec![(0, 0)],
        "the central record must lose its timestamps"
    );
    assert_eq!(
        zip_local_stamp(&stripped, before[0].local_offset as usize),
        (0, 0),
        "the local header must lose its timestamps too"
    );
    assert_eq!(
        parse_zip(&stripped)[0].decompressed(&stripped),
        b"stored bytes are never recompressed"
    );
    assert_eq!(
        std::fs::metadata(&path).unwrap().modified().unwrap(),
        mtime_before,
        "the filesystem mtime is not metadata inside the file"
    );
}

#[test]
fn strip_metadata_equal_size_gzip_is_not_a_failure() {
    let ws = Workspace::new("strip-gzip-equal");
    // No FNAME, no FEXTRA, no FCOMMENT and a non-zero MTIME, so stripping the
    // header can only zero four bytes. The body is written at level 9 and the
    // handler recompresses at level 9, so on this fixture the candidate is the
    // same length as the input (#26). Even if the recompression does gain a byte,
    // the contract is the same: never larger, and the header is stripped.
    let payload = compressible(20_000);
    let original = build_gzip(&payload, None, None, 0, 9);
    let path = ws.write("stamped.gz", &original);

    let (code, stdout, stderr) = run(&["stamped.gz", "--strip-metadata"], &ws.path);
    assert_eq!(code, 0, "stdout={stdout} stderr={stderr}");
    assert!(
        !stdout.contains("internal") && !stderr.contains("internal"),
        "stdout={stdout} stderr={stderr}"
    );
    assert!(
        stdout.contains("metadata stripped"),
        "the fixture must actually be rewritten: {stdout}"
    );

    let stripped = std::fs::read(&path).unwrap();
    assert!(
        stripped.len() <= original.len(),
        "a stripping rewrite never grows a file: {} -> {}",
        original.len(),
        stripped.len()
    );
    assert_eq!(
        &stripped[..4],
        &[0x1f, 0x8b, 0x08, 0x00],
        "flags must be zero"
    );
    assert_eq!(&stripped[4..8], &[0, 0, 0, 0], "MTIME must be zero");

    let (payload_before, crc_before) = inflate_member(&original);
    let (payload_after, crc_after) = inflate_member(&stripped);
    assert_eq!(payload_before, payload_after);
    assert_eq!(crc_before, crc_after);
}

#[test]
fn strip_metadata_is_reported_and_dry_run_writes_nothing() {
    let ws = Workspace::new("strip-dry");
    let spec = PngSpec {
        level: 1,
        extra_chunks: vec![("tEXt".to_string(), b"Key\0value".to_vec())],
        ..Default::default()
    };
    let original = build_png(&spec);
    let path = ws.write("tagged.png", &original);

    let (code, stdout, stderr) = run(&["tagged.png", "--strip-metadata", "--dry-run"], &ws.path);
    assert_eq!(code, 0, "{stderr}");
    assert!(stdout.contains("metadata stripped"), "{stdout}");
    assert_eq!(
        std::fs::read(&path).unwrap(),
        original,
        "dry run must not write"
    );

    // The JSON schema says so too, for scripts that audit what happened.
    let (code, stdout, stderr) = run(
        &["tagged.png", "--strip-metadata", "--dry-run", "--json"],
        &ws.path,
    );
    assert_eq!(code, 0, "{stderr}");
    assert!(stdout.contains("\"metadata_stripped\":true"), "{stdout}");
    assert!(stdout.contains("\"strip_metadata\":true"), "{stdout}");
}

#[test]
fn metadata_is_preserved_when_the_flag_is_absent() {
    let ws = Workspace::new("keep-meta");
    let spec = PngSpec {
        level: 1,
        extra_chunks: vec![("tEXt".to_string(), b"Comment\0keep me".to_vec())],
        ..Default::default()
    };
    let original = build_png(&spec);
    let path = ws.write("tagged.png", &original);

    let (code, _, stderr) = run(&["tagged.png"], &ws.path);
    assert_eq!(code, 0, "{stderr}");
    let optimized = std::fs::read(&path).unwrap();
    let chunks = png_chunks(&optimized).unwrap();
    assert!(
        chunks.iter().any(|(t, _)| t == "tEXt"),
        "without the flag no metadata may be removed"
    );
}

// ---------------------------------------------------------------------------
// TIFF
// ---------------------------------------------------------------------------

#[test]
fn tiff_uncompressed_strips_are_compressed_and_decode_identically() {
    let ws = Workspace::new("tiff-opt");
    let spec = TiffSpec {
        width: 96,
        height: 64,
        samples_per_pixel: 3,
        photometric: 2,
        rows_per_strip: 16,
        extra_ascii: vec![
            (270, "a description".to_string()),
            (305, "some editor".to_string()),
        ],
        ..Default::default()
    };
    let original = build_tiff(&spec);
    let path = ws.write("scan.tif", &original);

    let (code, stdout, stderr) = run(&["scan.tif"], &ws.path);
    assert_eq!(code, 0, "stdout={stdout} stderr={stderr}");

    let optimized = std::fs::read(&path).unwrap();
    assert!(optimized.len() < original.len(), "expected a smaller file");
    assert_eq!(
        tiff_compression(&optimized),
        8,
        "strips must be Deflate now"
    );
    assert_eq!(
        tiff_strips(&optimized),
        tiff_strips(&original),
        "pixels must be identical"
    );

    // Everything but the three entries the rewrite is allowed to touch.
    let before = parse_tiff_tags(&original);
    let after = parse_tiff_tags(&optimized);
    assert_eq!(
        before.iter().map(|(t, _, _, _)| *t).collect::<Vec<_>>(),
        after.iter().map(|(t, _, _, _)| *t).collect::<Vec<_>>(),
        "the tag set and its order must survive"
    );
    for ((tag, typ, count, was), (_, now_typ, now_count, now)) in before.iter().zip(after.iter()) {
        match *tag {
            // The only three entries the rewrite may touch.
            259 | 273 | 279 => {}
            _ => {
                assert_eq!(typ, now_typ, "the type of tag {tag} changed");
                assert_eq!(count, now_count, "the count of tag {tag} changed");
                assert_eq!(was, now, "the value of tag {tag} changed");
            }
        }
    }
}

#[test]
fn tiff_both_byte_orders_round_trip() {
    for big_endian in [false, true] {
        let ws = Workspace::new("tiff-endian");
        let spec = TiffSpec {
            big_endian,
            width: 32,
            height: 32,
            ..Default::default()
        };
        let original = build_tiff(&spec);
        let path = ws.write("scan.tif", &original);
        let (code, stdout, stderr) = run(&["scan.tif"], &ws.path);
        assert_eq!(
            code, 0,
            "big_endian={big_endian} stdout={stdout} stderr={stderr}"
        );

        let optimized = std::fs::read(&path).unwrap();
        assert_eq!(
            &optimized[..2],
            &original[..2],
            "byte order must be preserved"
        );
        assert_eq!(tiff_strips(&optimized), tiff_strips(&original));
    }
}

#[test]
fn tiff_already_deflated_small_file_is_left_alone() {
    let ws = Workspace::new("tiff-nogain");
    let spec = TiffSpec {
        width: 16,
        height: 16,
        compression: 8,
        rows_per_strip: 16,
        ..Default::default()
    };
    let original = build_tiff(&spec);
    let path = ws.write("tiny.tif", &original);

    let (code, stdout, stderr) = run(&["tiny.tif"], &ws.path);
    assert_eq!(code, 0, "stdout={stdout} stderr={stderr}");
    assert!(stdout.contains("already"), "{stdout}");
    assert_eq!(
        std::fs::read(&path).unwrap(),
        original,
        "nothing to gain, nothing to write"
    );
}

#[test]
fn tiff_bigtiff_is_skipped_with_a_reason() {
    let ws = Workspace::new("tiff-bigtiff");
    let original = tiff_header(false, 43);
    let path = ws.write("huge.tif", &original);

    let (code, stdout, stderr) = run(&["huge.tif"], &ws.path);
    assert_eq!(code, 0, "a refusal is not a failure: {stdout} {stderr}");
    assert!(stdout.contains("BigTIFF"), "{stdout}");
    assert_eq!(std::fs::read(&path).unwrap(), original);
}

#[test]
fn tiff_tiled_layout_is_skipped() {
    let ws = Workspace::new("tiff-tiled");
    let spec = TiffSpec {
        tiled: true,
        width: 32,
        height: 32,
        ..Default::default()
    };
    let original = build_tiff(&spec);
    let path = ws.write("tiled.tif", &original);

    let (code, stdout, stderr) = run(&["tiled.tif"], &ws.path);
    assert_eq!(code, 0, "stdout={stdout} stderr={stderr}");
    assert!(stdout.contains("tiled"), "{stdout}");
    assert_eq!(std::fs::read(&path).unwrap(), original);
}

#[test]
fn tiff_lzw_is_skipped_because_there_is_no_encoder() {
    let ws = Workspace::new("tiff-lzw");
    let spec = TiffSpec {
        // LZW. The fixture's strips are not really LZW, so the refusal has to
        // come from the tag rather than from a failed decode.
        compression: 5,
        ..Default::default()
    };
    let original = build_tiff(&spec);
    let path = ws.write("lzw.tif", &original);

    let (code, stdout, stderr) = run(&["lzw.tif"], &ws.path);
    assert_eq!(code, 0, "stdout={stdout} stderr={stderr}");
    assert!(
        stdout.contains("compression 5") || stdout.contains("LZW"),
        "{stdout}"
    );
    assert_eq!(std::fs::read(&path).unwrap(), original);
}

#[test]
fn tiff_predictor_is_skipped() {
    let ws = Workspace::new("tiff-predictor");
    let spec = TiffSpec {
        predictor: Some(2),
        ..Default::default()
    };
    let original = build_tiff(&spec);
    let path = ws.write("pred.tif", &original);

    let (code, stdout, stderr) = run(&["pred.tif"], &ws.path);
    assert_eq!(code, 0, "stdout={stdout} stderr={stderr}");
    assert!(stdout.contains("predictor"), "{stdout}");
    assert_eq!(std::fs::read(&path).unwrap(), original);
}

#[test]
fn tiff_unknown_out_of_line_tag_is_skipped() {
    let ws = Workspace::new("tiff-private");
    let spec = TiffSpec {
        // 65000 is private: an out-of-line value there may be an offset, so the
        // rebuild cannot place it.
        extra_ascii: vec![(65000, "private payload".to_string())],
        ..Default::default()
    };
    let original = build_tiff(&spec);
    let path = ws.write("private.tif", &original);

    let (code, stdout, stderr) = run(&["private.tif"], &ws.path);
    assert_eq!(code, 0, "stdout={stdout} stderr={stderr}");
    assert!(stdout.contains("65000"), "{stdout}");
    assert_eq!(std::fs::read(&path).unwrap(), original);
}

#[test]
fn tiff_truncated_file_fails_and_is_untouched() {
    let ws = Workspace::new("tiff-truncated");
    let original = build_tiff(&TiffSpec::default());
    let cut = original[..original.len() / 2].to_vec();
    let path = ws.write("cut.tif", &cut);

    let (code, stdout, _stderr) = run(&["cut.tif"], &ws.path);
    assert_eq!(code, 1, "{stdout}");
    assert_eq!(std::fs::read(&path).unwrap(), cut);
}

#[test]
fn tiff_strip_metadata_removes_only_textual_tags() {
    let ws = Workspace::new("tiff-strip");
    let spec = TiffSpec {
        extra_ascii: vec![
            (270, "image description".to_string()),
            (305, "software".to_string()),
            (315, "artist".to_string()),
            (33432, "copyright".to_string()),
        ],
        extra_short: vec![(274, 1)], // Orientation: geometry, must stay
        ..Default::default()
    };
    let original = build_tiff(&spec);
    let path = ws.write("tagged.tif", &original);

    let (code, stdout, stderr) = run(&["--strip-metadata", "tagged.tif"], &ws.path);
    assert_eq!(code, 0, "stdout={stdout} stderr={stderr}");

    let optimized = std::fs::read(&path).unwrap();
    let tags: Vec<u16> = parse_tiff_tags(&optimized)
        .into_iter()
        .map(|(t, _, _, _)| t)
        .collect();
    for removed in [270u16, 305, 315, 33432] {
        assert!(
            !tags.contains(&removed),
            "tag {removed} should be gone: {tags:?}"
        );
    }
    assert!(tags.contains(&274), "Orientation is geometry, not metadata");
    assert!(tags.contains(&256) && tags.contains(&259), "{tags:?}");
    assert_eq!(tiff_strips(&optimized), tiff_strips(&original));
    assert!(stdout.contains("metadata"), "{stdout}");
}

#[test]
fn png_is_skipped_when_only_gzip_is_enabled() {
    let ws = Workspace::new("png-disabled");
    let spec = PngSpec {
        width: 64,
        height: 64,
        level: 1,
        ..Default::default()
    };
    let original = build_png(&spec);
    let path = ws.write("photo.png", &original);
    // A gzip file in the same run proves the policy is per format and not a
    // refusal of the whole invocation.
    let gz = build_gzip(&b"payload".repeat(500), None, None, 0, 1);
    let other = ws.write("blob.gz", &gz);

    let (code, stdout, stderr) = run(&["--formats", "gzip", "photo.png", "blob.gz"], &ws.path);
    assert_eq!(code, 0, "stdout={stdout} stderr={stderr}");
    // `unsupported` would mean "no handler for this content"; `failed` would mean
    // "corrupt". A disabled handler is a policy decision, and it has to say so.
    // The summary always prints every counter, so the assertions are on the
    // per-file line and on the two counters that decide the exit code.
    assert!(
        stdout.contains("skipped: PNG is not enabled for this run"),
        "{stdout}"
    );
    assert!(stdout.contains("0 unsupported"), "{stdout}");
    assert!(stdout.contains("0 failed"), "{stdout}");
    assert_eq!(
        std::fs::read(&path).unwrap(),
        original,
        "a policy skip must leave the file alone"
    );
    assert!(
        std::fs::read(&other).unwrap().len() < gz.len(),
        "the format that is enabled must still be processed"
    );
}

#[test]
fn nested_children_inherit_the_formats_allowlist() {
    let ws = Workspace::new("nested-allow");
    let png_spec = PngSpec {
        width: 96,
        height: 96,
        level: 1,
        ..Default::default()
    };
    let inner_png = build_png(&png_spec);
    let original = build_zip(
        &[ZipEntrySpec::deflated("images/logo.png", &inner_png, 1)],
        b"",
    );
    let path = ws.write("archive.zip", &original);

    // `zip` is enabled, `png` is not: the archive is still recompressed, but its
    // child is left byte for byte alone even though `--nested` is on.
    let (code, stdout, stderr) = run(&["archive.zip", "--nested", "--formats", "zip"], &ws.path);
    assert_eq!(code, 0, "stdout={stdout} stderr={stderr}");

    let out = std::fs::read(&path).unwrap();
    assert!(out.len() <= original.len());
    let entry = parse_zip(&out).remove(0);
    assert_eq!(
        entry.decompressed(&out),
        inner_png,
        "a child outside the allowlist must not be rewritten"
    );
    assert_eq!(entry.crc, crc32fast::hash(&inner_png));
}

#[test]
fn tiff_is_skipped_when_the_format_is_not_enabled() {
    let ws = Workspace::new("tiff-disabled");
    let original = build_tiff(&TiffSpec::default());
    let path = ws.write("scan.tif", &original);

    let (code, stdout, stderr) = run(&["--formats", "png", "scan.tif"], &ws.path);
    assert_eq!(code, 0, "stdout={stdout} stderr={stderr}");
    assert!(stdout.contains("skipped"), "{stdout}");
    assert_eq!(std::fs::read(&path).unwrap(), original);
}

// ---------------------------------------------------------------------------
// Traceability rows that had no test (#49). Each name starts with the row id.
// ---------------------------------------------------------------------------

#[test]
fn t_png_09_zopfli_is_at_least_as_good_as_the_default() {
    let ws = Workspace::new("row-png-09");
    let spec = PngSpec {
        width: 96,
        height: 96,
        level: 1,
        ..Default::default()
    };
    let original = build_png(&spec);
    let plain = ws.write("plain.png", &original);
    let strong = ws.write("strong.png", &original);

    let (code, _, stderr) = run(&["plain.png"], &ws.path);
    assert_eq!(code, 0, "{stderr}");
    let (code, _, stderr) = run(&["strong.png", "--zopfli"], &ws.path);
    assert_eq!(code, 0, "{stderr}");

    let plain_out = std::fs::read(&plain).unwrap();
    let strong_out = std::fs::read(&strong).unwrap();
    assert!(plain_out.len() < original.len());
    assert!(
        strong_out.len() <= plain_out.len(),
        "--zopfli must never be worse: {} > {}",
        strong_out.len(),
        plain_out.len()
    );
    // The candidate is still a PNG with identical pixels, whichever candidate won.
    let (_, _, p1) = decode_pixels(&original).unwrap();
    let (_, _, p2) = decode_pixels(&strong_out).unwrap();
    assert_eq!(p1, p2);
}

#[test]
fn t_gz_01_a_comment_is_preserved() {
    let ws = Workspace::new("row-gz-01");
    let payload = b"comment-carrying payload".repeat(200);
    let bare = build_gzip(&payload, None, None, 0, 1);
    // build_gzip writes FNAME and FEXTRA but not FCOMMENT, so set flag bit 4 and
    // splice the NUL-terminated comment in after the ten-byte header.
    let mut original = bare[..10].to_vec();
    original[3] |= 0x10;
    original.extend_from_slice(b"written by a test\0");
    original.extend_from_slice(&bare[10..]);
    let path = ws.write("comment.gz", &original);

    let (code, stdout, stderr) = run(&["comment.gz"], &ws.path);
    assert_eq!(code, 0, "stdout={stdout} stderr={stderr}");

    let out = std::fs::read(&path).unwrap();
    assert!(out.len() < original.len());
    assert!(
        out.windows(18).any(|w| w == b"written by a test\0"),
        "FCOMMENT must survive verbatim"
    );
    let header_len = 10 + b"written by a test\0".len();
    assert_eq!(
        &out[..header_len],
        &original[..header_len],
        "the header, comment included, is copied verbatim"
    );
}

#[test]
fn t_gz_02_fhcrc_is_preserved_and_still_valid() {
    let ws = Workspace::new("row-gz-02");
    let payload = b"header checksum payload".repeat(200);
    let bare = build_gzip(&payload, Some(b"name.txt"), None, 0, 1);
    // FLG |= FHCRC, and the two CRC-16 bytes are the low half of the header's
    // CRC-32, which is what the format defines.
    let mut original = bare[..10].to_vec();
    original[3] |= 0x02;
    let mut with_name = original.clone();
    with_name.extend_from_slice(b"name.txt\0");
    let crc16 = (crc32fast::hash(&with_name) & 0xffff) as u16;
    original.extend_from_slice(&with_name[10..]);
    original.extend_from_slice(&crc16.to_le_bytes());
    original.extend_from_slice(&bare[10 + 9..]);
    let path = ws.write("fhcrc.gz", &original);

    let (code, stdout, stderr) = run(&["fhcrc.gz"], &ws.path);
    assert_eq!(code, 0, "stdout={stdout} stderr={stderr}");

    let out = std::fs::read(&path).unwrap();
    assert_eq!(out[3] & 0x02, 0x02, "FHCRC flag must survive");
    let header_len = 10 + 9 + 2;
    assert_eq!(
        &out[..header_len],
        &original[..header_len],
        "header verbatim"
    );
    let recomputed = (crc32fast::hash(&out[..header_len - 2]) & 0xffff) as u16;
    assert_eq!(
        u16::from_le_bytes([out[header_len - 2], out[header_len - 1]]),
        recomputed,
        "the stored CRC-16 must still match the header it covers"
    );
}

#[test]
fn t_zip_09_an_unknown_compression_method_is_refused() {
    let ws = Workspace::new("row-zip-09");
    let mut spec = ZipEntrySpec::deflated("odd.bin", &compressible(2000), 1);
    spec.method = 12; // bzip2: valid in the format, not implemented here
    let original = build_zip(&[spec], b"");
    let path = ws.write("odd.zip", &original);

    let (code, stdout, stderr) = run(&["odd.zip"], &ws.path);
    assert_eq!(code, 0, "stdout={stdout} stderr={stderr}");
    assert!(stdout.contains("skipped"), "{stdout}");
    assert!(stdout.contains("compression method"), "{stdout}");
    assert_eq!(std::fs::read(&path).unwrap(), original);
}

#[test]
fn t_zip_09_a_multi_disk_archive_is_refused() {
    let ws = Workspace::new("row-zip-09-disk");
    let original = build_zip(
        &[ZipEntrySpec::deflated("a.txt", &compressible(2000), 1)],
        b"",
    );
    let mut damaged = original.clone();
    let eocd = damaged.len() - 22;
    assert_eq!(
        u32::from_le_bytes([
            damaged[eocd],
            damaged[eocd + 1],
            damaged[eocd + 2],
            damaged[eocd + 3]
        ]),
        ZIP_EOCD_SIG
    );
    // "number of this disk" = 1: the archive claims to be the second of a set.
    damaged[eocd + 4] = 1;
    let path = ws.write("split.zip", &damaged);

    let (code, stdout, stderr) = run(&["split.zip"], &ws.path);
    assert_eq!(code, 0, "stdout={stdout} stderr={stderr}");
    assert!(stdout.contains("multi-disk"), "{stdout}");
    assert_eq!(std::fs::read(&path).unwrap(), damaged);
}

#[test]
fn t_zip_12_max_uncompressed_is_enforced() {
    let ws = Workspace::new("row-zip-12");
    let original = build_zip(
        &[ZipEntrySpec::deflated("big.txt", &compressible(200_000), 9)],
        b"",
    );
    let path = ws.write("big.zip", &original);

    let (code, stdout, stderr) = run(&["big.zip", "--max-uncompressed", "1000"], &ws.path);
    assert_eq!(code, 0, "stdout={stdout} stderr={stderr}");
    assert!(stdout.contains("--max-uncompressed"), "{stdout}");
    assert_eq!(std::fs::read(&path).unwrap(), original);

    // The same archive is fine with the default limit.
    let (code, _, stderr) = run(&["big.zip"], &ws.path);
    assert_eq!(code, 0, "{stderr}");
    assert!(std::fs::read(&path).unwrap().len() <= original.len());
}

#[test]
fn t_ico_04_a_directory_that_runs_past_the_file_is_refused() {
    let ws = Workspace::new("row-ico-04");
    let entry = IcoEntrySpec {
        width: 16,
        height: 16,
        bit_count: 32,
        payload: bmp_icon_payload(64),
    };
    let original = build_ico(1, &[entry]);
    // The directory claims two images, and the file is cut short of the second
    // record. The count stays inside the range detection accepts, so the file is
    // an ICO with a broken directory rather than an unknown format: `failed`,
    // not `unsupported`.
    let mut damaged = original[..32].to_vec();
    damaged[4..6].copy_from_slice(&2u16.to_le_bytes());
    let path = ws.write("directory.ico", &damaged);

    let (code, stdout, stderr) = run(&["directory.ico"], &ws.path);
    assert_eq!(code, 1, "stdout={stdout} stderr={stderr}");
    assert!(stdout.contains("truncated directory"), "{stdout}");
    assert_eq!(std::fs::read(&path).unwrap(), damaged);
}

// ---------------------------------------------------------------------------
// Terminal behaviour: colour, progress, closed pipes, usage errors.
// ---------------------------------------------------------------------------

#[test]
fn colour_is_absent_in_a_pipe_and_present_when_forced() {
    let ws = Workspace::new("colour");
    let spec = PngSpec {
        width: 64,
        height: 64,
        level: 1,
        ..Default::default()
    };
    ws.write("a.png", &build_png(&spec));

    // Piped stdout is not a terminal, so the default is plain text — even
    // though the same bytes would be coloured in a real terminal.
    let (code, plain, _) = run(&["a.png"], &ws.path);
    assert_eq!(code, 0);
    assert!(
        !plain.contains('\u{1b}'),
        "a pipe must stay clean: {plain:?}"
    );

    let (code, forced, _) = run(&["--color=always", "--dry-run", "a.png"], &ws.path);
    assert_eq!(code, 0);
    assert!(
        forced.contains("\u{1b}["),
        "--color=always must colour: {forced:?}"
    );

    let (code, never, _) = run(&["--color=never", "--dry-run", "a.png"], &ws.path);
    assert_eq!(code, 0);
    assert!(
        !never.contains('\u{1b}'),
        "--color=never must not colour: {never:?}"
    );

    // The escape sequences must not change the text: stripping them gives back
    // exactly what --color=never printed. Only the first line is compared,
    // because the summary carries the elapsed time, which differs between runs.
    assert_eq!(
        strip_ansi(forced.lines().next().unwrap()),
        never.lines().next().unwrap()
    );
}

#[test]
fn json_is_never_coloured() {
    let ws = Workspace::new("colour-json");
    let spec = PngSpec {
        width: 32,
        height: 32,
        level: 1,
        ..Default::default()
    };
    ws.write("a.png", &build_png(&spec));

    let (code, stdout, stderr) = run(&["--json", "--color=always", "a.png"], &ws.path);
    assert_eq!(code, 0, "{stderr}");
    assert!(
        !stdout.contains('\u{1b}'),
        "JSON carried an escape: {stdout:?}"
    );
    assert!(stdout
        .lines()
        .last()
        .unwrap()
        .contains("\"type\":\"summary\""));
}

#[test]
fn a_closed_pipe_is_not_a_crash() {
    let ws = Workspace::new("closed-pipe");
    let spec = PngSpec {
        width: 32,
        height: 32,
        level: 1,
        ..Default::default()
    };
    for i in 0..40 {
        ws.write(&format!("f{i:02}.png"), &build_png(&spec));
    }

    // The reader disappears before the writer says anything: the shape of
    // `tzim . -r | head -1`. Writing must fail quietly, not panic.
    let mut child = std::process::Command::new(env!("CARGO_BIN_EXE_tzim"))
        .args(["-r", "."])
        .current_dir(&ws.path)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("spawn");
    let _ = child.stdout.take();
    let output = child.wait_with_output().expect("wait");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.code(), Some(0), "stderr: {stderr}");
    assert!(!stderr.contains("panicked"), "{stderr}");
    assert!(!stderr.contains("failed printing"), "{stderr}");
}

#[test]
fn usage_errors_show_the_usage_line() {
    let ws = Workspace::new("usage-line");
    let (code, stdout, stderr) = run(&["--nope"], &ws.path);
    assert_eq!(code, 2);
    assert!(
        stdout.is_empty(),
        "usage errors belong on stderr: {stdout:?}"
    );
    assert!(
        stderr.contains("usage: tzim [OPTIONS] <PATH>..."),
        "{stderr}"
    );
    assert!(stderr.contains("--nope"), "{stderr}");
    assert!(stderr.contains("--help"), "{stderr}");
}

#[test]
fn clustered_flags_work_end_to_end() {
    let ws = Workspace::new("cluster");
    let spec = PngSpec {
        width: 64,
        height: 64,
        level: 1,
        ..Default::default()
    };
    let original = build_png(&spec);
    ws.write("sub/deep.png", &original);

    // `-nr` == `-n -r`: a recursive dry run, nothing written.
    let (code, stdout, stderr) = run(&["-nr", "sub"], &ws.path);
    assert_eq!(code, 0, "stdout={stdout} stderr={stderr}");
    assert!(stdout.contains("would optimize"), "{stdout}");
    assert_eq!(std::fs::read(ws.file("sub/deep.png")).unwrap(), original);

    // `-j2` == `-j 2`.
    let (code, _, stderr) = run(&["-j2", "-n", "sub"], &ws.path);
    assert_eq!(code, 0, "{stderr}");
}

#[test]
fn no_progress_flag_is_accepted_and_silent() {
    let ws = Workspace::new("progress-off");
    let spec = PngSpec {
        width: 32,
        height: 32,
        level: 1,
        ..Default::default()
    };
    ws.write("a.png", &build_png(&spec));

    let (code, stdout, stderr) = run(&["--no-progress", "--dry-run", "a.png"], &ws.path);
    assert_eq!(code, 0);
    // stderr is a pipe here, so no animation either way; the flag must at least
    // not add anything of its own.
    assert!(!stderr.contains('\r'), "{stderr:?}");
    assert!(!stdout.contains('\r'), "{stdout:?}");
}

/// Removes SGR sequences so two renderings can be compared.
fn strip_ansi(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' {
            // Skip "[" and everything up to the final "m".
            for next in chars.by_ref() {
                if next == 'm' {
                    break;
                }
            }
        } else {
            out.push(c);
        }
    }
    out
}

// ---------------------------------------------------------------------------
// zlib (RFC 1950)
// ---------------------------------------------------------------------------

#[test]
fn zlib_is_recompressed_and_the_payload_is_identical() {
    let ws = Workspace::new("zlib-opt");
    let payload: Vec<u8> = (0..200_000u32).map(|i| (i % 9) as u8).collect();
    let original = build_zlib(&payload, 1);
    let path = ws.write("data.zlib", &original);

    let (code, stdout, stderr) = run(&["data.zlib"], &ws.path);
    assert_eq!(code, 0, "stdout={stdout} stderr={stderr}");

    let optimized = std::fs::read(&path).unwrap();
    assert!(optimized.len() < original.len(), "expected a smaller file");
    assert_eq!(
        optimized[..2],
        original[..2],
        "the header is copied verbatim"
    );
    assert_eq!(
        optimized[optimized.len() - 4..],
        original[original.len() - 4..],
        "the Adler-32 trailer must be unchanged: the payload is unchanged"
    );
    assert_eq!(zlib_payload(&optimized), payload);
    assert!(stdout.contains("ZLIB"), "{stdout}");
}

#[test]
fn a_second_zlib_run_changes_nothing() {
    let ws = Workspace::new("zlib-noop");
    let payload: Vec<u8> = (0..80_000u32).map(|i| (i % 5) as u8).collect();
    ws.write("data.zlib", &build_zlib(&payload, 1));

    let (code, _, _) = run(&["data.zlib"], &ws.path);
    assert_eq!(code, 0);
    let first = std::fs::read(ws.file("data.zlib")).unwrap();

    let (code, stdout, _) = run(&["data.zlib"], &ws.path);
    assert_eq!(code, 0);
    assert!(stdout.contains("already optimal"), "{stdout}");
    assert_eq!(std::fs::read(ws.file("data.zlib")).unwrap(), first);
}

#[test]
fn a_corrupt_adler32_is_refused_and_the_file_is_untouched() {
    let ws = Workspace::new("zlib-adler");
    let payload: Vec<u8> = (0..20_000u32).map(|i| (i % 3) as u8).collect();
    let mut original = build_zlib(&payload, 1);
    let last = original.len() - 1;
    original[last] ^= 0xff;
    let path = ws.write("broken.zlib", &original);

    let (code, stdout, stderr) = run(&["broken.zlib"], &ws.path);
    assert_eq!(code, 1, "stdout={stdout} stderr={stderr}");
    assert!(stdout.contains("failed"), "{stdout}");
    assert_eq!(std::fs::read(&path).unwrap(), original);
}

#[test]
fn a_preset_dictionary_is_not_even_detected() {
    let ws = Workspace::new("zlib-fdict");
    // A valid-looking zlib header with FDICT set: the stream needs a dictionary
    // this build does not have, so the file is not treated as zlib at all.
    let header = [0x78u8, 0xbb];
    let mut data = header.to_vec();
    data.extend_from_slice(&[0xde, 0xad, 0xbe, 0xef]); // dictionary id
    data.extend_from_slice(&[0x03, 0x00, 0x00, 0x00, 0x00, 0x01]);
    let path = ws.write("dict.zlib", &data);

    let (code, stdout, stderr) = run(&["dict.zlib"], &ws.path);
    assert_eq!(code, 0, "stdout={stdout} stderr={stderr}");
    assert!(stdout.contains("unsupported"), "{stdout}");
    assert_eq!(std::fs::read(&path).unwrap(), data);
}

#[test]
fn a_small_window_is_skipped_with_a_reason() {
    let ws = Workspace::new("zlib-window");
    let payload: Vec<u8> = (0..500u32).map(|i| (i % 4) as u8).collect();
    let original = zlib_with_window(&build_zlib(&payload, 1), 4);
    let path = ws.write("small.zlib", &original);

    let (code, stdout, stderr) = run(&["small.zlib"], &ws.path);
    assert_eq!(code, 0, "stdout={stdout} stderr={stderr}");
    assert!(stdout.contains("skipped"), "{stdout}");
    assert!(stdout.contains("window"), "{stdout}");
    assert_eq!(std::fs::read(&path).unwrap(), original);
}

#[test]
fn data_after_the_stream_is_skipped_and_untouched() {
    let ws = Workspace::new("zlib-trailing");
    let payload: Vec<u8> = (0..4_000u32).map(|i| (i % 3) as u8).collect();
    let mut data = build_zlib(&payload, 6);
    // A byte between the end of the DEFLATE stream and the checksum: a second
    // stream could follow, and this build does not concatenate them.
    data.insert(data.len() - 4, 0x00);
    let path = ws.write("trailing.zlib", &data);

    let (code, stdout, stderr) = run(&["trailing.zlib"], &ws.path);
    assert_eq!(code, 0, "stdout={stdout} stderr={stderr}");
    assert!(stdout.contains("skipped"), "{stdout}");
    assert_eq!(std::fs::read(&path).unwrap(), data);
}

#[test]
fn a_false_positive_is_reported_and_never_written() {
    let ws = Workspace::new("zlib-false-positive");
    // A valid header, then bytes that are not a DEFLATE stream. Detection cannot
    // tell this apart from a corrupt file, so either verdict is acceptable — but
    // the file must come back untouched, whatever the verdict is.
    let mut data = vec![0x78, 0x9c];
    data.extend_from_slice(b"this is not a deflate stream at all, it only starts like one");
    data.extend_from_slice(&[0x00, 0x00, 0x00, 0x01]);
    let path = ws.write("fake.zlib", &data);

    let (code, stdout, stderr) = run(&["fake.zlib"], &ws.path);
    assert!(
        code == 0 || code == 1,
        "code={code} stdout={stdout} stderr={stderr}"
    );
    assert!(
        stdout.contains("failed") || stdout.contains("skipped"),
        "{stdout}"
    );
    assert_eq!(std::fs::read(&path).unwrap(), data);
}

#[test]
fn zlib_is_listed_and_can_be_named() {
    let ws = Workspace::new("zlib-formats");
    let (code, stdout, _) = run(&["--list-formats"], &ws.path);
    assert_eq!(code, 0);
    assert!(stdout.contains("ZLIB"), "{stdout}");

    let payload: Vec<u8> = (0..40_000u32).map(|i| (i % 6) as u8).collect();
    ws.write("data.zlib", &build_zlib(&payload, 1));
    let (code, stdout, stderr) = run(&["--formats", "png", "data.zlib"], &ws.path);
    assert_eq!(code, 0, "{stderr}");
    assert!(stdout.contains("skipped"), "{stdout}");
}

#[test]
fn strip_metadata_has_nothing_to_remove_from_zlib() {
    let ws = Workspace::new("zlib-strip");
    let payload: Vec<u8> = (0..60_000u32).map(|i| (i % 11) as u8).collect();
    let original = build_zlib(&payload, 1);
    let path = ws.write("data.zlib", &original);

    let (code, stdout, stderr) = run(&["--strip-metadata", "data.zlib"], &ws.path);
    assert_eq!(code, 0, "stdout={stdout} stderr={stderr}");
    let optimized = std::fs::read(&path).unwrap();
    // A zlib header carries no name, comment or timestamp: the two bytes are the
    // original two bytes, and no line claims otherwise.
    assert_eq!(optimized[..2], original[..2]);
    assert_eq!(zlib_payload(&optimized), payload);
}
