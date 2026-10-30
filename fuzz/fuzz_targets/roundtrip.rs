#![no_main]

use libfuzzer_sys::fuzz_target;

// Format detection, the handler for the detected format, and verification of the
// candidate the handler produced. The fuzz workspace enables every optional
// handler. The checked-in workflow supplies JPEG, PNG/TIFF, zlib and fixed FLAC
// seeds plus a deterministic valid PDF seed; it does not claim dedicated seed
// coverage for gzip, ZIP/ICO or FITS.
//
// The additional property checked here is the contract itself: when
// `roundtrip` returns true, a candidate was produced, it is smaller than the
// input and it passed verification. The function never asserts on the input, so
// any panic is a defect.
fuzz_target!(|data: &[u8]| {
    let _ = tzim::fuzz_api::roundtrip(data);
});
