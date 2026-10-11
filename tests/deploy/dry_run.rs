//! The hub's own side of a safe deploy, with the real binary: `trommi-hub dry-run` (a new release tried on a copy
//! of the live data while the live hub runs, the live data never written), and the mark `hub.served` that tells
//! hub/deploy/hub-restore.sh whether data may still go back to the copy made before a release started.

use std::io::{BufRead, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::Duration;

use serde_json::Value;
use sha2::{Digest, Sha256};

/// The hub of this workspace (`TROMMI_HUB_BIN`, or built beside the tests).
fn hub_binary() -> PathBuf {
    if let Some(path) = std::env::var_os("TROMMI_HUB_BIN") {
        return PathBuf::from(path);
    }
    static BUILT: std::sync::Once = std::sync::Once::new();
    let profile = std::env::current_exe()
        .unwrap()
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf();
    BUILT.call_once(|| {
        let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
        let mut build = Command::new(cargo);
        build.args(["build", "--locked", "-p", "trommi-hub"]);
        if profile.file_name().is_some_and(|d| d == "release") {
            build.arg("--release");
        }
        assert!(
            build.stdout(Stdio::null()).status().unwrap().success(),
            "the hub builds"
        );
    });
    profile.join("trommi-hub")
}

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "trommi-dryrun-{name}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

struct Hub {
    child: Child,
    port: u16,
}

impl Hub {
    fn start(data: &Path) -> Hub {
        let mut child = Command::new(hub_binary())
            .env("HUB_DATA", data)
            .env("HUB_HOST", "127.0.0.1")
            .env("HUB_PORT", "0")
            .env("HUB_QUIET", "1")
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let mut line = String::new();
        std::io::BufReader::new(child.stdout.take().unwrap())
            .read_line(&mut line)
            .unwrap();
        let port = serde_json::from_str::<Value>(&line).unwrap()["port"]
            .as_u64()
            .unwrap() as u16;
        Hub { child, port }
    }
}

impl Drop for Hub {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn get(port: u16, path: &str) -> (u16, Value) {
    let mut s = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
    s.write_all(format!("GET {path} HTTP/1.1\r\nhost: x\r\nconnection: close\r\n\r\n").as_bytes())
        .unwrap();
    let mut answer = String::new();
    s.read_to_string(&mut answer).unwrap();
    let status = answer.split_whitespace().nth(1).unwrap().parse().unwrap();
    let body = answer.split_once("\r\n\r\n").map_or("", |b| b.1);
    let json = match (body.find('{'), body.rfind('}')) {
        (Some(a), Some(b)) => serde_json::from_str(&body[a..=b]).unwrap_or(Value::Null),
        _ => Value::Null,
    };
    (status, json)
}

fn dry_run(data: &Path, copy: &Path) -> (bool, Value) {
    let out = Command::new(hub_binary())
        .args([
            "dry-run",
            data.to_str().unwrap(),
            copy.to_str().unwrap(),
            "30",
        ])
        .stderr(Stdio::null())
        .output()
        .unwrap();
    let line = String::from_utf8_lossy(&out.stdout);
    let verdict: Value = serde_json::from_str(line.lines().last().unwrap_or("")).unwrap();
    (out.status.success(), verdict)
}

/// Every file of the folder with its SHA-256.
fn snapshot(dir: &Path) -> Vec<(String, String)> {
    let mut all: Vec<(String, String)> = std::fs::read_dir(dir)
        .unwrap()
        .flatten()
        .filter(|e| e.path().is_file())
        .map(|e| {
            (
                e.file_name().to_string_lossy().to_string(),
                format!("{:x}", Sha256::digest(std::fs::read(e.path()).unwrap())),
            )
        })
        .collect();
    all.sort();
    all
}

fn set_read_only(dir: &Path, read_only: bool) {
    use std::os::unix::fs::PermissionsExt;
    let mode = if read_only { 0o500 } else { 0o700 };
    std::fs::set_permissions(dir, std::fs::Permissions::from_mode(mode)).unwrap();
    for e in std::fs::read_dir(dir).unwrap().flatten() {
        if e.path().is_file() {
            let mode = if read_only { 0o400 } else { 0o600 };
            std::fs::set_permissions(e.path(), std::fs::Permissions::from_mode(mode)).unwrap();
        }
    }
}

#[test]
fn a_release_is_tried_on_a_copy_while_the_live_hub_runs_and_the_live_data_is_not_written() {
    let dir = scratch("good");
    let data = dir.join("data");
    let live = Hub::start(&data);
    assert_eq!(get(live.port, "/healthz").0, 200);
    let before = snapshot(&data);
    // as under its unit: the live data only to read
    set_read_only(&data, true);
    let (ok, verdict) = dry_run(&data, &dir.join("copy"));
    set_read_only(&data, false);
    assert!(ok, "{verdict}");
    assert_eq!(verdict["ok"], true);
    assert!(verdict["tables"].as_u64().unwrap() > 10, "{verdict}");
    assert!(verdict["copied_bytes"].as_u64().unwrap() > 0);
    assert!(!dir.join("copy").exists(), "the copy is removed");
    assert_eq!(snapshot(&data), before, "the live data is as it was");
    // the live hub never noticed
    assert_eq!(get(live.port, "/healthz").1["ok"], true);
    drop(live);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_database_this_hub_cannot_open_fails_the_dry_run_and_is_left_alone() {
    let dir = scratch("bad");
    let data = dir.join("data");
    let hub = Hub::start(&data);
    assert_eq!(get(hub.port, "/healthz").0, 200);
    drop(hub);
    // a database from a later hub with another schema: this one refuses it at its start
    let c = rusqlite::Connection::open(data.join("hub.db")).unwrap();
    c.execute_batch("PRAGMA user_version = 99;").unwrap();
    drop(c);
    let before = snapshot(&data);
    // as under its unit, and with no hub running (no -wal file): read as it lies
    set_read_only(&data, true);
    let (ok, verdict) = dry_run(&data, &dir.join("copy"));
    set_read_only(&data, false);
    assert!(!ok);
    assert_eq!(verdict["ok"], false);
    assert!(
        verdict["detail"].as_str().unwrap().contains("schema 99"),
        "{verdict}"
    );
    assert_eq!(snapshot(&data), before);
    assert!(!dir.join("copy").exists());
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn the_hub_marks_its_data_before_its_first_request_and_says_so_after_a_restart() {
    let dir = scratch("served");
    let data = dir.join("data");
    let hub = Hub::start(&data);
    // asking for its health is no request that changes data
    for _ in 0..3 {
        assert_eq!(get(hub.port, "/healthz").1["served"], false);
    }
    assert!(!data.join("hub.served").exists());
    let (status, _) = get(hub.port, "/v1/shares/AAAAAAAAAAAAAAAAAAAAAA");
    assert!(status < 500);
    assert!(data.join("hub.served").exists());
    assert_eq!(get(hub.port, "/healthz").1["served"], true);
    drop(hub);
    let again = Hub::start(&data);
    assert_eq!(get(again.port, "/healthz").1["served"], true);
    drop(again);
    let _ = std::fs::remove_dir_all(dir);
}
