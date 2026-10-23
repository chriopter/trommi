//! The conformance scripts of the app core (core/client/tests/conformance/*.json) through this app's own fold:
//! the App answers, `shell::View` folds each render, and the JSON the window would read is what is compared
//! (core/client/README.md, "Conformance").

use std::path::Path;
use trommi_client_core::app::script::{run, Script, Shell};
use trommi_client_core::app::{App, Effect, Event};
use trommi_linux::shell::View;

#[derive(Default)]
struct LinuxShell {
    app: App,
    view: View,
}

impl Shell for LinuxShell {
    fn send(&mut self, now_ms: u64, event: Event) -> Vec<Effect> {
        let effects = self.app.handle(now_ms, event);
        for effect in &effects {
            if let Effect::Render { delta, .. } = effect {
                assert!(self.view.fold(delta), "a render came out of order");
            }
        }
        effects
    }

    fn view(&self) -> serde_json::Value {
        serde_json::from_str(&self.view.json()).expect("the window reads JSON")
    }
}

#[test]
fn every_script_holds_through_the_linux_fold() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../core/client/tests/conformance");
    let mut scripts: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|e| e == "json"))
        .collect();
    scripts.sort();
    assert!(!scripts.is_empty(), "no script in {}", dir.display());
    for path in scripts {
        let script: Script = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        if let Err(m) = run(&script, &mut LinuxShell::default()) {
            panic!("{}: {m}", path.display());
        }
    }
}

/// The recorded view scenes (core/client/tests/conformance/views/*.json, `view_scenes.rs`): every delta a shell
/// was sent, folded by this app's own fold from an empty model, reaches the recorded view exactly, as the window
/// reads it.
#[test]
fn every_view_scene_folds_to_its_view_through_the_linux_fold() {
    use trommi_client_core::views::ViewDelta;
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../core/client/tests/conformance/views");
    let mut scenes: Vec<_> = std::fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("{}: {e}", dir.display()))
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|e| e == "json"))
        .collect();
    scenes.sort();
    assert!(!scenes.is_empty(), "no scene in {}", dir.display());
    for path in scenes {
        let scene: serde_json::Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        let mut view = View::default();
        let deltas = scene["deltas"].as_array().unwrap_or_else(|| panic!("{}: no deltas", path.display()));
        assert!(!deltas.is_empty(), "{}: no deltas", path.display());
        for (i, delta) in deltas.iter().enumerate() {
            let delta: ViewDelta = serde_json::from_value(delta.clone()).unwrap_or_else(|e| panic!("{} delta {i}: {e}", path.display()));
            assert!(view.fold(&delta), "{} delta {i}: the fold refused it", path.display());
        }
        let got: serde_json::Value = serde_json::from_str(&view.json()).expect("the window reads JSON");
        assert_eq!(got, scene["view"], "{}: the window would read another view", path.display());
    }
}
