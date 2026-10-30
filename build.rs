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
    if value { "true" } else { "false" }
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
    write!(
        json,
        "{{\"format\":\"jpeg\",\"available\":{},\
         \"kind\":\"independent_verifier_default_gate\",\"build_feature\":\"jpeg\",\
         \"default_admission_feature\":\"jpeg-v1\",\"default_enabled\":{},\
         \"codec_only_selection_flag\":\"--formats jpeg\"}},",
        bool_text(enabled("jpeg")),
        bool_text(enabled("jpeg-v1")),
    )
    .unwrap();
    write!(
        json,
        "{{\"format\":\"fits\",\"available\":{},\
         \"kind\":\"visible_transform_consent\",\"build_feature\":\"fits\",\
         \"runtime_opt_in_flag\":\"--fits-tile\",\"default_enabled\":false,\
         \"allowlist_is_not_consent\":true}},",
        bool_text(enabled("fits")),
    )
    .unwrap();
    write!(
        json,
        "{{\"format\":\"flac\",\"available\":{},\
         \"kind\":\"optional_build_feature\",\"build_feature\":\"flac\",\
         \"feature_in_default_set\":false,\"enabled_by_default_when_compiled\":true,\
         \"runtime_opt_in_flag\":null}},",
        bool_text(enabled("flac")),
    )
    .unwrap();
    write!(
        json,
        "{{\"format\":\"pdf\",\"available\":{},\
         \"kind\":\"optional_build_feature\",\"build_feature\":\"pdf\",\
         \"feature_in_default_set\":false,\"enabled_by_default_when_compiled\":true,\
         \"runtime_opt_in_flag\":null}}]",
        bool_text(enabled("pdf")),
    )
    .unwrap();

    json.push_str(
        ",\"transformations\":[{\"flag\":\"--strip-metadata\",\
         \"available\":true,\"default\":false,\
         \"effect\":\"removes only the documented per-format metadata list\"},\
         {\"flag\":\"--nested\",\"available\":true,\"default\":false,\
         \"effect\":\"also considers supported files inside archives\"},",
    );
    write!(
        json,
        "{{\"flag\":\"--zopfli\",\"available\":{},\"default\":false,\
         \"effect\":\"uses the slower, stronger DEFLATE encoder where supported\"}},",
        bool_text(enabled("zopfli")),
    )
    .unwrap();
    write!(
        json,
        "{{\"flag\":\"--fits-tile\",\"available\":{},\"default\":false,\
         \"effect\":\"converts an eligible FITS IMAGE HDU to a tile-compressed BINTABLE\",\
         \"confirmation_policy\":\"fits_tile_confirmation\"}}]",
        bool_text(enabled("fits")),
    )
    .unwrap();

    write!(
        json,
        ",\"confirmation_policies\":[{{\"id\":\"fits_tile_confirmation\",\
         \"flag\":\"--fits-tile\",\"available\":{},\
         \"interactive\":{{\"eligible_for_single_direct_regular_file_only\":true,\
         \"requires_stdin_and_stderr_tty\":true,\"prompt_stream\":\"stderr\",\
         \"default_answer\":\"no\",\"runs_after_candidate_verification\":true,\
         \"only_yes_commits\":true}},\
         \"non_interactive\":{{\"prompt\":false,\"flag_is_explicit_opt_in\":true}},\
         \"decline\":{{\"status\":\"skipped\",\"original_unchanged\":true}},\
         \"never_prompts_for\":[\"--json\",\"--dry-run\",\"recursive_or_batch_runs\",\
         \"nested_work\",\"filtered_or_refused_inputs\",\
         \"candidate_or_verification_failures\"]}}],\
         \"side_effects\":{{\"scans_files\":false,\"writes_files\":false,\"prompts\":false}}}}\n",
        bool_text(enabled("fits")),
    )
    .unwrap();
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
