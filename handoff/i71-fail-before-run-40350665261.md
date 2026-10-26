<!-- tzim-ci:test-ubuntu-latest-all-features:a399776 -->
### CI failure: `test-ubuntu-latest-all-features`

```
test encrypted_zip_is_refused ... ok
test fits_tests::fits_a_file_that_would_grow_is_left_alone ... ok
test directory_is_processed_recursively ... ok
test dry_run_does_not_modify_anything ... ok
test fits_tests::fits_an_unsupported_tile_codec_is_skipped ... ok
test fits_tests::fits_a_float_array_uses_gzip_2_and_round_trips ... ok
test fits_tests::fits_does_nothing_without_the_flag ... ok
test fits_tests::fits_a_checksummed_hdu_is_kept_and_the_rest_is_rewritten ... ok
test fits_tests::fits_an_image_hdu_becomes_a_compressed_table_that_round_trips ... ok
test fits_tests::fits_random_groups_use_the_legal_data_length_and_are_skipped ... FAILED
test fits_tests::fits_is_listed_and_can_be_named ... ok
test fits_tests::fits_decimal_and_exponent_identity_scaling_is_accepted_exactly ... ok
test fits_tests::fits_range_sensitive_non_identity_scaling_is_refused_exactly ... FAILED
test fits_tests::fits_simple_false_is_refused_without_rewriting ... FAILED
test formats_flag_restricts_other_formats_too ... ok
test fits_tests::fits_every_other_hdu_is_copied_verbatim ... ok
test gzip_header_is_preserved_byte_for_byte ... ok
test help_and_version_succeed ... ok
test gzip_payload_is_unchanged ... ok
test ico_with_a_bad_offsets_is_refused ... ok
test ico_with_only_bitmaps_is_already_optimal ... ok
test include_and_exclude_filters ... ok
test info_answers_without_touching_anything ... ok
test info_names_the_jpeg_verification_level_of_this_build ... ok
test ico_embedded_pngs_are_optimized_and_bitmaps_preserved ... ok
test jpeg_arithmetic_is_skipped ... ok
test jpeg_grayscale_is_optimized ... ok
test jpeg_is_inert_without_the_formats_flag ... ok
test jpeg_baseline_is_optimized_and_then_stable ... ok
test jpeg_progressive_is_skipped ... ok
test fits_tests::fits_structural_cards_and_comments_are_encoded_verbatim_in_z_forms ... FAILED
test jpeg_trailing_bytes_after_eoi_are_preserved ... ok
test jpeg_truncated_file_fails_and_is_untouched ... ok
test jpeg_twelve_bit_is_skipped ... ok
test jpeg_restart_intervals_are_preserved ... ok
test fits_tests::fits_the_two_shapes_the_reference_writer_lost_are_kept ... ok
test json_is_never_coloured ... ok
test json_output_has_one_record_per_file_and_a_summary ... ok
test metadata_is_preserved_when_the_flag_is_absent ... ok
test missing_path_fails_but_others_continue ... ok
test nested_does_not_touch_signed_archives ... ok
test no_progress_flag_is_accepted_and_silent ... ok
test nested_children_inherit_the_formats_allowlist ... ok
test nested_depth_limit_is_respected ... ok
test permissions_and_mtime_survive_the_replacement ... ok
test png_is_skipped_when_only_gzip_is_enabled ... ok
test png_ancillary_and_private_chunks_are_preserved ... ok
test read_only_file_is_skipped ... ok
test signed_jar_is_refused_and_left_untouched ... ok
test signed_odf_document_is_refused ... ok
test strip_metadata_clears_the_gzip_header_and_keeps_the_payload ... ok
test nested_optimization_reaches_the_file_inside_the_archive ... ok
test strip_metadata_drops_zip_comments_and_keeps_extra_fields ... ok
test strip_metadata_drops_jpeg_exif_and_comment ... ok
test strip_metadata_equal_size_zip_is_optimized ... ok
test strip_metadata_equal_size_gzip_is_not_a_failure ... ok
test png_is_optimized_and_pixels_are_identical ... ok
test strip_metadata_has_nothing_to_remove_from_zlib ... ok
test strip_metadata_is_reported_and_dry_run_writes_nothing ... ok
test stub_prefixed_archive_is_not_detected_as_zip ... ok
test symlinks_are_not_followed_by_default ... ok
test t_apng_05_an_inconsistent_frame_sequence_is_refused ... ok
test t_fs_12_a_device_node_is_skipped ... ok
test t_fs_11_a_fifo_is_skipped_and_never_opened ... ok
test t_fs_14_a_symlink_cycle_terminates_and_is_reported ... ok
test strip_metadata_removes_png_text_and_exif_but_keeps_the_content ... ok
test second_run_is_a_no_op ... ok
test t_gz_01_a_comment_is_preserved ... ok
test t_fs_15_a_read_only_directory_is_a_failure_and_the_file_survives ... ok
test t_ico_04_a_directory_that_runs_past_the_file_is_refused ... ok
test t_gz_02_fhcrc_is_preserved_and_still_valid ... ok
test t_fs_16_a_tree_deeper_than_sixty_four_levels_is_refused ... ok
test t_fs_13_a_full_disk_is_a_failure_with_nothing_left_behind ... ok
test t_zip_09_a_multi_disk_archive_is_refused ... ok
test t_zip_09_an_unknown_compression_method_is_refused ... ok
test t_png_04_an_interlaced_image_is_recompressed_and_pixels_are_identical ... ok
test t_zip_12_max_uncompressed_is_enforced ... ok
test tiff_already_deflated_small_file_is_left_alone ... ok
test tiff_bigtiff_is_skipped_with_a_reason ... ok
test tiff_both_byte_orders_round_trip ... ok
test tiff_fill_order_two_with_deflate_says_the_file_cannot_be_repaired ... ok
test tiff_is_skipped_when_the_format_is_not_enabled ... ok
test tiff_lzw_is_skipped_because_there_is_no_encoder ... ok
test tiff_predictor_is_skipped ... ok
test tiff_reversed_bit_order_is_skipped ... ok
test tiff_strip_metadata_removes_only_textual_tags ... ok
test tiff_tiled_layout_is_skipped ... ok
test tiff_truncated_file_fails_and_is_untouched ... ok
test tiff_uncompressed_strips_are_compressed_and_decode_identically ... ok
test tiff_unknown_out_of_line_tag_is_skipped ... ok
test truncated_zip_is_refused ... ok
test the_memory_budget_boundary_is_exact ... ok
test unsupported_files_are_reported_and_untouched ... ok
test usage_errors_exit_with_two ... ok
test unicode_and_space_filenames_work ... ok
test usage_errors_show_the_usage_line ... ok
test zip64_is_refused ... ok
test zip_data_descriptor_is_rewritten_consistently ... ok
test t_png_03_every_scanline_filter_round_trips ... ok
test zip_deflate_entries_are_recompressed_and_everything_else_is_preserved ... ok
test zip_non_utf8_names_are_preserved ... ok
test zip_stored_entries_stay_stored ... ok
test zlib_is_listed_and_can_be_named ... ok
test zlib_is_recompressed_and_the_payload_is_identical ... ok
test zip_bomb_ratio_is_refused ... ok
test t_png_09_zopfli_is_at_least_as_good_as_the_default ... ok

failures:

---- fits_tests::fits_random_groups_use_the_legal_data_length_and_are_skipped stdout ----

thread 'fits_tests::fits_random_groups_use_the_legal_data_length_and_are_skipped' (2992) panicked at tests/integration.rs:3670:9:
assertion `left == right` failed: stdout=groups.fits  FITS   failed: fits: a second primary HDU
Summary: 1 files, 0 optimized, 0 B saved, 0 already optimal, 0 unsupported, 0 skipped, 1 failed, 0.00 s
 stderr=
  left: 1
 right: 0
note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace

---- fits_tests::fits_range_sensitive_non_identity_scaling_is_refused_exactly stdout ----

thread 'fits_tests::fits_range_sensitive_non_identity_scaling_is_refused_exactly' (2995) panicked at tests/integration.rs:3820:13:
rounded-one: scaled.fits  FITS   64.69 KiB -> 30.94 KiB  -52.2%
Summary: 1 files, 1 optimized, 33.75 KiB saved, 0 already optimal, 0 unsupported, 0 skipped, 0 failed, 0.05 s


---- fits_tests::fits_simple_false_is_refused_without_rewriting stdout ----

thread 'fits_tests::fits_simple_false_is_refused_without_rewriting' (2998) panicked at tests/integration.rs:3693:9:
the documented refusal must be named: not-simple.fits  FITS   64.69 KiB -> 30.94 KiB  -52.2%
Summary: 1 files, 1 optimized, 33.75 KiB saved, 0 already optimal, 0 unsupported, 0 skipped, 0 failed, 0.04 s


---- fits_tests::fits_structural_cards_and_comments_are_encoded_verbatim_in_z_forms stdout ----

thread 'fits_tests::fits_structural_cards_and_comments_are_encoded_verbatim_in_z_forms' (3001) panicked at tests/integration.rs:3768:17:
assertion `left == right` failed: SIMPLE must survive as the complete ZSIMPLE card
  left: "ZSIMPLE =                    T                                                  "
 right: "ZSIMPLE =                    T / original primary logical comment               "


failures:
    fits_tests::fits_random_groups_use_the_legal_data_length_and_are_skipped
    fits_tests::fits_range_sensitive_non_identity_scaling_is_refused_exactly
    fits_tests::fits_simple_false_is_refused_without_rewriting
    fits_tests::fits_structural_cards_and_comments_are_encoded_verbatim_in_z_forms

test result: FAILED. 130 passed; 4 failed; 0 ignored; 0 measured; 0 filtered out; finished in 14.88s

error: test failed, to rerun pass `--test integration`
```

