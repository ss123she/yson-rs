#![no_main]

//! Binary format: no input may panic or hang.

use libfuzzer_sys::fuzz_target;
use yson_rs::{Reader, YsonFormat, YsonValue, from_slice, scan_value};

fuzz_target!(|data: &[u8]| {
    let format = YsonFormat::Binary;
    let _ = scan_value(data, format);
    let _ = Reader::new(data, format).read_value();
    let _ = from_slice::<YsonValue>(data, format);
});
