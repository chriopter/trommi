//! The shell's loop as the window starts it: the folders of the slot, the App behind them, and the view's JSON
//! handed to the window after every render.

use std::sync::mpsc;
use std::time::Duration;
use trommi_linux::shell;

fn phase(json: &str) -> String {
    let view: serde_json::Value = serde_json::from_str(json).expect("the window reads JSON");
    view["phase"].as_str().unwrap_or("").to_string()
}

/// Waits until the window was handed a view in this phase.
fn wait_for(drawn: &mpsc::Receiver<String>, want: &str) {
    loop {
        let json = drawn.recv_timeout(Duration::from_secs(5)).unwrap_or_else(|_| panic!("no view in the phase {want}"));
        if phase(&json) == want {
            return;
        }
    }
}

#[test]
fn the_window_is_handed_the_sign_in_and_the_slot_has_one_owner() {
    let dir = std::env::temp_dir().join(format!("trommi-linux-shell-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::env::set_var("TROMMI_LINUX_DIR", &dir);
    let runtime = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap();

    let (draw, drawn) = mpsc::channel();
    let handle = shell::start(&runtime, shell::config(120), move |json| {
        let _ = draw.send(json);
    })
    .unwrap();
    // nothing is stored: the core says so, and the window draws the sign-in
    wait_for(&drawn, "signed_out");
    assert!(dir.join("default/device").is_dir() && dir.join("default/cache").is_dir());

    // the app started twice: the second has no slot
    let second = shell::start(&runtime, shell::config(120), |_| {});
    assert_eq!(second.err().as_deref(), Some("another process holds this state"));

    // the app ends (its handle goes), and starts again over the same folders
    drop(handle);
    let mut again = None;
    for _ in 0..100 {
        let (draw, drawn) = mpsc::channel();
        match shell::start(&runtime, shell::config(120), move |json| {
            let _ = draw.send(json);
        }) {
            Ok(handle) => {
                again = Some((handle, drawn));
                break;
            }
            Err(_) => std::thread::sleep(Duration::from_millis(20)),
        }
    }
    let (_handle, drawn) = again.expect("the slot is free once the loop ended");
    wait_for(&drawn, "signed_out");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_config_is_this_platforms() {
    let config = shell::config(-300);
    assert_eq!((config.platform.as_str(), config.slot.as_str(), config.utc_offset_min), ("linux", "default", -300));
    assert!(config.seed < 1 << 53);
    assert!(!config.locale.contains('.') && !config.locale.contains('_'));
}

#[test]
fn the_golden_views_fold_into_what_the_window_reads() {
    let golden = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../core/client/tests/golden");
    for (sidebar, desk) in [("demo-sidebar.json", "demo-desk.json"), ("demo-sidebar-all.json", "demo-desk-all.json"), ("demo-sidebar-platform.json", "demo-desk-platform.json")] {
        let read = |name: &str| std::fs::read_to_string(golden.join(name)).unwrap();
        let json = shell::golden(&read(sidebar), &read(desk)).unwrap();
        let view: serde_json::Value = serde_json::from_str(&json).unwrap();
        // what the window reads is what the core gave: nothing is added, dropped or reordered on the way
        let given: serde_json::Value = serde_json::from_str(&read(sidebar)).unwrap();
        assert_eq!(view["sidebar"], given, "{sidebar}");
        let given: serde_json::Value = serde_json::from_str(&read(desk)).unwrap();
        assert_eq!(view["desk"], given, "{desk}");
        assert_eq!(view["phase"], "live");
    }
    assert!(shell::golden("{", "{}").is_err());
}
