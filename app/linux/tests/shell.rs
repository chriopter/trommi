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
fn the_window_sends_only_the_persons_events() {
    let ok = |json: &str| shell::person_event(json).is_ok();
    assert!(ok(r#"{"type":"focus","focus":{"screen":{"screen":"card","card_id":"ab"}}}"#));
    assert!(ok(r#"{"type":"action","action":{"do":"star","card_id":"ab","on":true}}"#));
    assert!(ok(r#"{"type":"foreground"}"#) && ok(r#"{"type":"background"}"#) && ok(r#"{"type":"zone","utc_offset_min":60}"#));
    // what the loop alone sends
    assert!(!ok(r#"{"type":"wake"}"#));
    assert!(!ok(r#"{"type":"start","config":{"platform":"linux","version":"0","seed":1}}"#));
    assert!(!ok(r#"{"type":"store","id":1,"result":{"ok":{"kind":"done"}}}"#));
    assert!(!ok("{"));
}

#[test]
fn the_desktop_does_what_the_core_asks() {
    use std::os::unix::fs::PermissionsExt;
    use std::sync::{Arc, Mutex};
    use trommi_client_core::app::{Effect, Notice};
    use trommi_client_core::views::Text;
    use trommi_io::Shell;
    use trommi_linux::platform::Desktop;

    let dir = std::env::temp_dir().join(format!("trommi-linux-desktop-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let mut desktop = Desktop::with_dirs(dir.join("files"), dir.join("Downloads"));
    let seen: Arc<Mutex<Vec<String>>> = Arc::default();
    let s = seen.clone();
    desktop.on_files = Box::new(move |json| s.lock().unwrap().push(format!("files {json}")));
    let s = seen.clone();
    desktop.on_saved = Box::new(move |path| s.lock().unwrap().push(format!("saved {}", path.file_name().unwrap().to_string_lossy())));
    let s = seen.clone();
    desktop.on_notice = Box::new(move |n| s.lock().unwrap().push(format!("notice {} {:?}", n.title.en, n.target)));
    let s = seen.clone();
    desktop.on_open = Box::new(move |url| s.lock().unwrap().push(format!("open {url}")));
    let mut window = shell::Window::with_desktop(|_| {}, desktop);

    // a file for drawing: kept where only this user reads it, named in the window's map; then dropped
    window.show(Effect::FileReady { file_id: "f/1".into(), media_type: "image/png".into(), bytes: b"png".to_vec().into() });
    let kept = dir.join("files/f%2F1");
    assert_eq!(std::fs::read(&kept).unwrap(), b"png");
    assert_eq!(std::fs::metadata(&kept).unwrap().permissions().mode() & 0o777, 0o600);
    assert_eq!(std::fs::metadata(dir.join("files")).unwrap().permissions().mode() & 0o777, 0o700);
    window.show(Effect::FileGone { file_id: "f/1".into() });
    assert!(!kept.exists());
    // a file offered: into Downloads, under a free name, never out of it
    for _ in 0..2 {
        window.show(Effect::SaveFile { name: "../report.pdf".into(), media_type: "application/pdf".into(), bytes: b"pdf".to_vec().into(), window: 0 });
    }
    assert!(dir.join("Downloads/report.pdf").is_file() && dir.join("Downloads/report (2).pdf").is_file());
    // links: only the web's and mail
    for url in ["https://trommi.com/help", "file:///etc/passwd", "javascript:alert(1)", "mailto:a@b.c"] {
        window.show(Effect::OpenUrl { url: url.into(), window: 0 });
    }
    let text = |en: &str| Text { key: "t".into(), args: Default::default(), en: en.into() };
    window.show(Effect::Notify { notice: Notice { id: "n".into(), title: text("Web: Which hero?"), body: text("Two drafts"), target: Some("c1".into()), urgent: false } });
    let file = format!("file://{}", kept.display());
    assert_eq!(
        *seen.lock().unwrap(),
        vec![
            format!("files {{\"f/1\":\"{file}\"}}"),
            "files {}".to_string(),
            "saved report.pdf".into(),
            "saved report (2).pdf".into(),
            "open https://trommi.com/help".into(),
            "open mailto:a@b.c".into(),
            "notice Web: Which hero? Some(\"c1\")".into(),
        ]
    );
    assert!(shell::config(0).can.notify);
    drop(window);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_picked_file_is_read_for_the_core() {
    let path = std::env::temp_dir().join(format!("trommi-linux-pick-{} a.png", std::process::id()));
    std::fs::write(&path, b"img").unwrap();
    let url = format!("file://{}", path.display().to_string().replace(' ', "%20"));
    let (name, media, bytes) = trommi_linux::platform::read_picked(&url).unwrap();
    assert!(name.ends_with(" a.png"), "{name}");
    assert_eq!((media.as_str(), bytes.as_slice()), ("image/png", b"img".as_slice()));
    assert!(trommi_linux::platform::read_picked("file:///nonexistent/x").is_err());
    assert!(trommi_linux::platform::read_picked(&std::env::temp_dir().display().to_string()).is_err());
    let _ = std::fs::remove_file(path);
}

#[test]
fn the_sign_in_goes_to_the_core_as_an_account_action() {
    let event = shell::person_event(r#"{"type":"action","action":{"do":"account","action":{"step":"sign_in","email":"a@b.c","password":"pw"}}}"#).unwrap();
    // (the password never shows in a log line)
    assert!(!format!("{event:?}").contains("pw"));
    assert!(shell::person_event(r#"{"type":"action","action":{"do":"account","action":{"step":"sign_in_with_kit","email":"a@b.c","words":"w"}}}"#).is_ok());
}
