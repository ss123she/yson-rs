#![no_main]

//! Text format: no input may panic or hang.
//!
//! Text mode is the one with comments and escapes, so it is where the parser
//! has the most states to get wrong — a stray `/` once spun here forever.

use libfuzzer_sys::fuzz_target;
use yson_rs::{Reader, YsonFormat, YsonValue, from_slice, scan_value};

fuzz_target!(|data: &[u8]| {
    let format = YsonFormat::Text;
    let _ = scan_value(data, format);
    let _ = Reader::new(data, format).read_value();
    let _ = from_slice::<YsonValue>(data, format);
});
