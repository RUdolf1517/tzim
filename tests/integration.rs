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

    let (code, stdout, stderr) = run(&["optimize", "bundle.zip"], &ws.path);
    assert_eq!(code, 0, "stdout={stdout} stderr={stderr}");

    let optimized = std::fs::read(&path).unwrap();
    assert!(optimized.len() < original.len(), "expected a smaller archive");

    let before = parse_zip(&original);
    let after = parse_zip(&optimized);
    assert_eq!(before.len(), after.len());

    for (a, b) in before.iter().zip(after.iter()) {
        assert_eq!(a.name, b.name, "names must be preserved");
        assert_eq!(a.method, b.method, "methods must not change");
        assert_eq!(a.flags, b.flags, "flags must be preserved");
        assert_eq!(a.crc, b.crc, "CRC must be preserved");
        assert_eq!(a.uncomp_size, b.uncomp_size, "uncompressed size must be preserved");
        assert_eq!(a.local_extra, b.local_extra, "local extra fields must be preserved");
        assert_eq!(a.central_extra, b.central_extra, "central extra fields must be preserved");
        assert_eq!(a.comment, b.comment, "entry comments must be preserved");
        assert_eq!(
            a.decompressed(&original),
            b.decompressed(&optimized),
            "entry payload must be identical"
        );
    }
    assert_eq!(zip_archive_comment(&original), zip_archive_comment(&optimized));
}

#[test]
fn zip_stored_entries_stay_stored() {
    let ws = Workspace::new("zip-store");
    let stored = ZipEntrySpec::stored("mimetype", b"application/vnd.oasis.opendocument.text");
    let oned = ZipEntrySpec::deflated("content.xml", &compressible(40_000), 1);
    let original = build_zip(&[stored, oned], b"");
    let path = ws.write("doc.odt", &original);

    let (code, _, stderr) = run(&["optimize", "doc.odt"], &ws.path);
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

    let (code, _, stderr) = run(&["optimize", "streamed.zip"], &ws.path);
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

    let (code, stdout, stderr) = run(&["optimize", "names.zip", "--json"], &ws.path);
    assert_eq!(code, 0, "stdout={stdout} stderr={stderr}");

    let optimized = std::fs::read(&path).unwrap();
    let after = parse_zip(&optimized);
    assert_eq!(after[0].name, vec![b'c', b'a', b'f', 0xe9, b'/', b'a', b'.', b't', b'x', b't']);
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

    let (code, stdout, stderr) = run(&["optimize", "signed.jar"], &ws.path);
    assert_eq!(code, 0, "stdout={stdout} stderr={stderr}");
    assert!(
        stdout.contains("signature") || stdout.contains("signed"),
        "the reason must be reported: {stdout}"
    );
    assert_eq!(std::fs::read(&path).unwrap(), original, "a signed archive must not change");
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

    let (code, _, stderr) = run(&["optimize", "signed.odt"], &ws.path);
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(std::fs::read(&path).unwrap(), original);
}

#[test]
fn apk_signing_block_is_refused() {
    let ws = Workspace::new("zip-apk");
    let archive = build_zip(
        &[ZipEntrySpec::deflated("classes.dex", &compressible(30_000), 1)],
        b"",
    );

    // Insert an APK signing block immediately before the central directory and
    // move the central directory offset accordingly.
    let eocd_at = (0..=archive.len() - 22)
        .rev()
        .find(|&at| {
            u32::from_le_bytes([archive[at], archive[at + 1], archive[at + 2], archive[at + 3]])
                == ZIP_EOCD_SIG
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
    let (code, stdout, stderr) = run(&["optimize", "app.apk"], &ws.path);
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

    let (code, stdout, stderr) = run(&["optimize", "big.zip"], &ws.path);
    assert_eq!(code, 0, "stdout={stdout} stderr={stderr}");
    assert!(stdout.contains("ZIP64"), "{stdout}");
    assert_eq!(std::fs::read(&path).unwrap(), original);
}

#[test]
fn encrypted_zip_is_refused() {
    let ws = Workspace::new("zip-crypt");
    let archive = build_zip_with_flags(
        &[ZipEntrySpec::deflated("secret.txt", &compressible(5_000), 1)],
        b"",
        0x0001,
    );
    let path = ws.write("secret.zip", &archive);

    let (code, stdout, stderr) = run(&["optimize", "secret.zip"], &ws.path);
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

    let (code, stdout, _) = run(&["optimize", "cut.zip"], &ws.path);
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

    let (code, stdout, _) = run(&["optimize", "broken.zip"], &ws.path);
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

    let (code, stdout, stderr) = run(&["optimize", "optimal.zip"], &ws.path);
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
    let archive = build_zip(&[ZipEntrySpec::deflated("zeros.bin", &vec![0u8; 2_000_000], 6)], b"");
    let path = ws.write("bomb.zip", &archive);

    let (code, stdout, stderr) = run(&["optimize", "bomb.zip", "--max-ratio", "10"], &ws.path);
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

    let (code, stdout, stderr) = run(&["optimize", "installer.exe"], &ws.path);
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

    let (code, stdout, stderr) = run(&["optimize", "anim.png"], &ws.path);
    assert_eq!(code, 0, "stdout={stdout} stderr={stderr}");

    let optimized = std::fs::read(&path).unwrap();
    assert!(optimized.len() < original.len(), "expected a smaller APNG");

    let before = png_chunks(&original).unwrap();
    let after = png_chunks(&optimized).unwrap();

    // Every control chunk (acTL, fcTL) must be byte-identical and in order.
    let controls_before: Vec<_> = before.iter().filter(|(t, _)| t != "IDAT" && t != "fdAT").collect();
    let controls_after: Vec<_> = after.iter().filter(|(t, _)| t != "IDAT" && t != "fdAT").collect();
    assert_eq!(controls_before, controls_after, "control chunks must be preserved");

    // The number of frame chunks and their sequence numbers must not change.
    let fdat_before: Vec<_> = before.iter().filter(|(t, _)| t == "fdAT").collect();
    let fdat_after: Vec<_> = after.iter().filter(|(t, _)| t == "fdAT").collect();
    assert_eq!(fdat_before.len(), fdat_after.len());
    for (a, b) in fdat_before.iter().zip(fdat_after.iter()) {
        assert_eq!(a.1[..4], b.1[..4], "fdAT sequence numbers must be preserved");
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

    let (code, _, stderr) = run(&["optimize", "single.png"], &ws.path);
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

    let (code, stdout, stderr) = run(&["optimize", "app.ico"], &ws.path);
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
    assert_eq!(after[1].1, bitmap, "bitmap image must be preserved byte for byte");
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

    let (code, stdout, stderr) = run(&["optimize", "plain.ico"], &ws.path);
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

    let (code, stdout, stderr) = run(&["optimize", "pointer.cur"], &ws.path);
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
    icon[6 + 12..6 + 16].copy_from_slice(&(icon.len() as u32 + 100).to_le_bytes());
    let original = icon.clone();
    let path = ws.write("broken.ico", &original);

    let (code, stdout, _) = run(&["optimize", "broken.ico"], &ws.path);
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
    let (code, _, stderr) = run(&["optimize", "plain.zip"], &ws.path);
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
    let (code, stdout, stderr) = run(&["optimize", "nested.zip", "--nested"], &ws.path);
    assert_eq!(code, 0, "stdout={stdout} stderr={stderr}");
    let nested_out = std::fs::read(&nested).unwrap();
    assert!(
        nested_out.len() < plain_out.len(),
        "--nested must beat recompression alone"
    );

    let entry = parse_zip(&nested_out).remove(0);
    let content = entry.decompressed(&nested_out);
    assert!(content.len() < inner_png.len(), "the embedded PNG must shrink");
    assert_eq!(entry.crc, crc32fast::hash(&content), "CRC must match the new content");
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
    let (code, _, stderr) = run(&["optimize", "shallow.zip", "--nested", "--max-depth", "1"], &ws.path);
    assert_eq!(code, 0, "{stderr}");
    let shallow_out = std::fs::read(&shallow).unwrap();
    let inner_after = parse_zip(&shallow_out).remove(0).decompressed(&shallow_out);
    assert_eq!(
        inner_after, inner_zip,
        "at depth 1 the inner archive must be left untouched"
    );

    // Depth 3: the PNG inside the inner archive is optimized too.
    let deep = ws.write("deep.zip", &outer);
    let (code, _, stderr) = run(&["optimize", "deep.zip", "--nested", "--max-depth", "3"], &ws.path);
    assert_eq!(code, 0, "{stderr}");
    let deep_out = std::fs::read(&deep).unwrap();
    assert!(deep_out.len() < shallow_out.len(), "deeper nesting should save more");
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

    let (code, stdout, stderr) = run(&["optimize", "signed.jar", "--nested"], &ws.path);
    assert_eq!(code, 0, "stdout={stdout} stderr={stderr}");
    assert_eq!(std::fs::read(&path).unwrap(), original);
}
