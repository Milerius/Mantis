#![no_main]
use libfuzzer_sys::fuzz_target;
use mantis_fixed::FixedI64;

extern crate alloc;
use alloc::format;

fuzz_target!(|raw: i64| {
    let f = FixedI64::<6>::from_raw(raw);
    let s = format!("{f}");
    let parsed = FixedI64::<6>::from_str_decimal(&s);
    assert_eq!(parsed, Ok(f), "roundtrip failed for raw={raw}");
});
