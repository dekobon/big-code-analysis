//! Byte-exact tagged documentation collection and adversarial query properties.
#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let result = big_code_analysis_fuzz::documentation::exercise(data);
    assert!(result.is_ok(), "{result:?}");
});
