#![no_main]

use libfuzzer_sys::fuzz_target;

// The same round trip as `roundtrip`, with `--strip-metadata`: the handler
// removes its format's metadata and verification requires that exactly those
// bytes are gone and everything else is intact.
//
// New code that deletes data deserves its own target. A panic, a candidate that
// verification accepts while a stripped structure survives, or any difference in
// the content are all defects.
fuzz_target!(|data: &[u8]| {
    let _ = tzim::fuzz_api::roundtrip_strip(data);
});
