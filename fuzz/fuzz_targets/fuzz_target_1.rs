#![no_main]

use libfuzzer_sys::fuzz_target;
use tsz_heatshrink::*;

// chosen based on bar chart in 'average-compression-tsz-data.png'
const DEFAULT_WINDOW_BITS: u8 = 9;
const DEFAULT_LOOKAHEAD_BITS: u8 = 7;

fuzz_target!(|data: &[u8]| {
    let mut compressed = Vec::new();
    encode(
        DEFAULT_WINDOW_BITS,
        DEFAULT_LOOKAHEAD_BITS,
        &mut &*data,
        &mut compressed,
    );
    let mut decompressed = Vec::new();
    decode(
        DEFAULT_WINDOW_BITS,
        DEFAULT_LOOKAHEAD_BITS,
        &mut compressed.as_slice(),
        &mut decompressed,
    );
    assert_eq!(data, decompressed.as_slice());
});
