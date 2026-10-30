use std::env;
use std::fmt::Write as _;
use std::fs;
use std::path::PathBuf;

const BUILD_FEATURES: &[(&str, &str)] = &[
    ("default", "default"),
    ("png", "png"),
    ("gzip", "gzip"),
    ("zip", "zip"),
    ("ico", "ico"),
    ("jpeg", "jpeg"),
    ("jpeg-v1", "jpeg-v1"),
    ("tiff", "tiff"),
    ("zlib", "zlib"),
    ("fits", "fits"),
    ("flac", "flac"),
    ("pdf", "pdf"),
    ("zopfli", "zopfli"),
];

const FORMAT_FEATURES: &[(&str, &str)] = &[
    ("png", "png"),
    ("gzip", "gzip"),
    ("zip", "zip"),
    ("ico", "ico"),
    ("jpeg", "jpeg"),
    ("tiff", "tiff"),
    ("zlib", "zlib"),
    ("fits", "fits"),
    ("flac", "flac"),
    ("pdf", "pdf"),
];

const DEFAULT_FORMAT_FEATURES: &[(&str, &str)] = &[
    ("png", "png"),
    ("gzip", "gzip"),
    ("zip", "zip"),
    ("ico", "ico"),
    ("jpeg", "jpeg-v1"),
    ("tiff", "tiff"),
    ("zlib", "zlib"),
    ("flac", "flac"),
    ("pdf", "pdf"),
];

fn enabled(feature: &str) -> bool {
    let variable = format!(
        "CARGO_FEATURE_{}",
        feature.replace('-', "_").to_ascii_uppercase()
    );
    env::var_os(variable).is_some()
}

fn bool_text(value: bool) -> &'static str {
    if value {
        "true"
    } else {
        "false"
    }
}

fn write_feature_map(json: &mut String) {
    for (index, (name, feature)) in BUILD_FEATURES.iter().enumerate() {
        if index > 0 {
            json.push(',');
        }
        write!(json, "\"{name}\":{}", bool_text(enabled(feature))).unwrap();
    }
}

fn write_format_array(json: &mut String, formats: &[(&str, &str)]) {
    let mut first = true;
    json.push('[');
    for (name, feature) in formats {
        if enabled(feature) {
            if !first {
                json.push(',');
            }
            write!(json, "\"{name}\"").unwrap();
            first = false;
        }
    }
    json.push(']');
}

fn push_json_separator(json: &mut String, first: &mut bool) {
    if !*first {
        json.push(',');
    }
    *first = false;
}

fn build_capabilities() -> String {
    let version = env::var("CARGO_PKG_VERSION").expect("Cargo sets package version");
    let mut json = String::with_capacity(2048);
    write!(
        json,
        "{{\"type\":\"capabilities\",\"capabilities_schema_version\":1,\
         \"result_schema_version\":2,\"tzim_version\":\"{version}\",\
         \"build_features\":{{"
    )
    .unwrap();
    write_feature_map(&mut json);
    json.push_str("},\"compiled_formats\":");
    write_format_array(&mut json, FORMAT_FEATURES);
    json.push_str(",\"default_enabled_formats\":");
    write_format_array(&mut json, DEFAULT_FORMAT_FEATURES);

    json.push_str(
        ",\"selection_flags\":[{\"flag\":\"--formats\",\"semantics\":\"allowlist\",\
         \"scope\":\"compiled_formats\",\"default_set\":\"default_enabled_formats\",\
         \"does_not_grant_handler_opt_ins\":true}],\"handler_opt_ins\":[",
    );
    let mut first = true;
    if enabled("jpeg") {
        push_json_separator(&mut json, &mut first);
        write!(
            json,
            "{{\"format\":\"jpeg\",\"available\":true,\
             \"kind\":\"independent_verifier_default_gate\",\"build_feature\":\"jpeg\",\
             \"default_admission_feature\":\"jpeg-v1\",\"default_enabled\":{},\
             \"codec_only_selection_flag\":\"--formats jpeg\"}}",
            bool_text(enabled("jpeg-v1")),
        )
        .unwrap();
    }
    if enabled("fits") {
        push_json_separator(&mut json, &mut first);
        json.push_str(
            "{\"format\":\"fits\",\"available\":true,\
             \"kind\":\"visible_transform_consent\",\"build_feature\":\"fits\",\
             \"runtime_opt_in_flag\":\"--fits-tile\",\"default_enabled\":false,\
             \"allowlist_is_not_consent\":true}",
        );
    }
    if enabled("flac") {
        push_json_separator(&mut json, &mut first);
        json.push_str(
            "{\"format\":\"flac\",\"available\":true,\
             \"kind\":\"optional_build_feature\",\"build_feature\":\"flac\",\
             \"feature_in_default_set\":false,\"enabled_by_default_when_compiled\":true,\
             \"runtime_opt_in_flag\":null}",
        );
    }
    if enabled("pdf") {
        push_json_separator(&mut json, &mut first);
        json.push_str(
            "{\"format\":\"pdf\",\"available\":true,\
             \"kind\":\"optional_build_feature\",\"build_feature\":\"pdf\",\
             \"feature_in_default_set\":false,\"enabled_by_default_when_compiled\":true,\
             \"runtime_opt_in_flag\":null}",
        );
    }
    json.push(']');

    json.push_str(
        ",\"transformations\":[{\"flag\":\"--strip-metadata\",\
         \"available\":true,\"default\":false,\
         \"effect\":\"removes only the documented per-format metadata list\"},\
         {\"flag\":\"--nested\",\"available\":true,\"default\":false,\
         \"effect\":\"also considers supported files inside archives\"}",
    );
    if enabled("zopfli") {
        json.push_str(
            ",{\"flag\":\"--zopfli\",\"available\":true,\"default\":false,\
             \"effect\":\"uses the slower, stronger DEFLATE encoder where supported\"}",
        );
    }
    if enabled("fits") {
        json.push_str(
            ",{\"flag\":\"--fits-tile\",\"available\":true,\"default\":false,\
             \"effect\":\"converts an eligible FITS IMAGE HDU to a tile-compressed BINTABLE\",\
             \"confirmation_policy\":\"fits_tile_confirmation\"}",
        );
    }
    json.push(']');

    json.push_str(",\"confirmation_policies\":[");
    if enabled("fits") {
        json.push_str(
            "{\"id\":\"fits_tile_confirmation\",\"flag\":\"--fits-tile\",\
             \"available\":true,\
             \"interactive\":{\"eligible_for_single_direct_regular_file_only\":true,\
             \"requires_stdin_and_stderr_tty\":true,\"prompt_stream\":\"stderr\",\
             \"default_answer\":\"no\",\"runs_after_candidate_verification\":true,\
             \"only_yes_commits\":true},\
             \"non_interactive\":{\"prompt\":false,\"flag_is_explicit_opt_in\":true},\
             \"decline\":{\"status\":\"skipped\",\"original_unchanged\":true},\
             \"never_prompts_for\":[\"--json\",\"--dry-run\",\"recursive_or_batch_runs\",\
             \"nested_work\",\"filtered_or_refused_inputs\",\
             \"candidate_or_verification_failures\"]}",
        );
    }
    json.push_str(
        "],\"side_effects\":{\"scans_files\":false,\"writes_files\":false,\"prompts\":false}}",
    );
    json.push('\n');
    json
}

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    for (_, feature) in BUILD_FEATURES {
        let variable = format!(
            "CARGO_FEATURE_{}",
            feature.replace('-', "_").to_ascii_uppercase()
        );
        println!("cargo:rerun-if-env-changed={variable}");
    }
    println!("cargo:rerun-if-env-changed=CARGO_PKG_VERSION");

    let output_dir = PathBuf::from(env::var_os("OUT_DIR").expect("Cargo sets OUT_DIR"));
    let capabilities = build_capabilities();
    fs::write(
        output_dir.join("capabilities.json"),
        capabilities.as_bytes(),
    )
    .expect("write compile-time capabilities record");
}
