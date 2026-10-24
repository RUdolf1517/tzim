#![no_main]

use libfuzzer_sys::fuzz_target;

// The JPEG marker parser, the layout analysis and the entropy decoder. These are
// the stages that read attacker-controlled numbers — segment lengths, sampling
// factors, Huffman counts, restart intervals — and the stages where an unchecked
// length or a missing bounds test turns into a panic or an allocation.
//
// The property is simply: return, do not panic, do not allocate without bound.
fuzz_target!(|data: &[u8]| {
    let _ = tzim::fuzz_api::jpeg_decode(data);
});
