//! The fuzz harnesses of `trommi_tests::fuzz` as tests that run everywhere (README "Fuzzing"): every
//! reproducer under `tests/fuzz/regress/<target>/` and every seed under `tests/fuzz/seeds/<target>/`, then
//! random inputs through proptest. `FUZZ_CASES` sets how many (default 64 per target).

use proptest::prelude::*;
use std::path::Path;

fn cases() -> u32 {
    std::env::var("FUZZ_CASES")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(64)
}

/// Runs `run` on every file kept for `target`.
fn replay(target: &str, run: fn(&[u8])) {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("fuzz");
    for kind in ["regress", "seeds"] {
        let Ok(dir) = std::fs::read_dir(root.join(kind).join(target)) else {
            continue;
        };
        let mut files: Vec<_> = dir.flatten().map(|e| e.path()).collect();
        files.sort();
        for file in files {
            let data = std::fs::read(&file).expect("a kept input reads");
            eprintln!("{}", file.display());
            run(&data);
        }
    }
}

macro_rules! target {
    ($name:ident, $module:ident, $max:expr) => {
        mod $name {
            use super::*;

            #[test]
            fn kept_inputs() {
                replay(stringify!($name), trommi_tests::fuzz::$module::run);
            }

            proptest! {
                #![proptest_config(ProptestConfig { cases: cases(), failure_persistence: None, ..ProptestConfig::default() })]
                #[test]
                fn random_inputs(data in proptest::collection::vec(any::<u8>(), 0..$max)) {
                    trommi_tests::fuzz::$module::run(&data);
                }
            }
        }
    };
}

target!(core_decode, decode, 2048);
target!(board_merge, board, 4096);
target!(content_flow, flow, 4096);
target!(mls_devices, devices, 1024);
target!(hub_api, hub, 2048);
target!(connector_text, connector, 2048);

/// Writes the seeds of the corpus: `cargo test -p trommi-tests --test core_fuzz_props write_seeds -- --ignored`.
#[test]
#[ignore]
fn write_seeds() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("fuzz")
        .join("seeds");
    for (target, seeds) in [("core_decode", trommi_tests::fuzz::decode::seeds())] {
        let dir = root.join(target);
        std::fs::create_dir_all(&dir).expect("the seed folder");
        for (i, seed) in seeds.iter().enumerate() {
            std::fs::write(dir.join(format!("{i:03}")), seed).expect("a seed");
        }
    }
}

/// Runs one input through one harness, to look at it: `FUZZ_TARGET=<module> FUZZ_FILE=<path> cargo test -p
/// trommi-tests --test core_fuzz_props one_input -- --ignored --nocapture`.
#[test]
#[ignore]
fn one_input() {
    let target = std::env::var("FUZZ_TARGET").expect("FUZZ_TARGET");
    let data = std::fs::read(std::env::var("FUZZ_FILE").expect("FUZZ_FILE")).expect("the input");
    let run: fn(&[u8]) = match target.as_str() {
        "decode" => trommi_tests::fuzz::decode::run,
        "board" => trommi_tests::fuzz::board::run,
        "flow" => trommi_tests::fuzz::flow::run,
        "devices" => trommi_tests::fuzz::devices::run,
        "hub" => trommi_tests::fuzz::hub::run,
        "connector" => trommi_tests::fuzz::connector::run,
        other => panic!("no harness {other}"),
    };
    run(&data);
}
