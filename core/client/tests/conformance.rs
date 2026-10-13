//! Runs every conformance script (tests/conformance/*.json) against the core and its own fold of the renders: the
//! reference every shell's harness matches (core/client/README.md, "Conformance").

use std::path::Path;
use trommi_client_core::app::script::{run, CoreShell, Script};

#[test]
fn every_script_holds_on_the_core() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/conformance");
    let mut names: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    names.sort();
    assert!(!names.is_empty());
    for path in names
        .into_iter()
        .filter(|p| p.extension().is_some_and(|e| e == "json"))
    {
        let script: Script = serde_json::from_slice(&std::fs::read(&path).unwrap())
            .unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        if let Err(m) = run(&script, &mut CoreShell::default()) {
            panic!("{}: {m}", path.display());
        }
    }
}
