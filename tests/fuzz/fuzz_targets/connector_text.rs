#![no_main]
libfuzzer_sys::fuzz_target!(|data: &[u8]| trommi_tests::fuzz::connector::run(data));
