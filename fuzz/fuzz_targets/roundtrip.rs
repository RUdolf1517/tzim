#![no_main]

use libfuzzer_sys::fuzz_target;

// Format detection, the handler for the detected format, and verification of the
// candidate the handler produced. This covers the PNG chunk walker, the gzip
// header/body splitter, the ZIP central-directory reader and the ICO directory on
// top of what `jpeg_decode` reaches.
//
// The additional property checked here is the contract itself: when
// `roundtrip` returns true, a candidate was produced, it is smaller than the
// input and it passed verification. The function never asserts on the input, so
// any panic is a defect.
fuzz_target!(|data: &[u8]| {
    let _ = tzim::fuzz_api::roundtrip(data);
});
