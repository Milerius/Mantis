#![no_main]
use libfuzzer_sys::fuzz_target;
use mantis_fixed::FixedI64;

fuzz_target!(|data: &str| {
    // Must never panic, always returns Ok or Err.
    let _ = FixedI64::<2>::from_str_decimal(data);
    let _ = FixedI64::<4>::from_str_decimal(data);
    let _ = FixedI64::<6>::from_str_decimal(data);
    let _ = FixedI64::<8>::from_str_decimal(data);
});
