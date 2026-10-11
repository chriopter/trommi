//! `trommi-hub dry-run <data> <scratch>`: a new release tried on a copy of the live data before it replaces the
//! running hub (hub/deploy/hub-dryrun.sh runs it as the hub's user, without network and with the live data
//! read-only; the updater asks for it before every swap).
//!
//! 1. A consistent copy of the database while the running hub goes on writing (`VACUUM INTO` from a read-only
//!    connection), and of the hub's own keys (`*.key`), into `<scratch>`. Uploaded files are not copied.
//! 2. This very program started as a hub on that copy (`HUB_DRY_RUN=1`: no push leaves, no periodic job runs, no
//!    admin page), on 127.0.0.1 and a port the system picks. Its start opens the database and runs the migrations.
//! 3. It must answer `/healthz` as well and as this commit, and a route under `/v1/` without a server error.
//! 4. It is stopped; the migrated copy must pass SQLite's `quick_check` and `foreign_key_check`, carry this
//!    hub's schema version, and every table must be readable (its rows counted).
//!
//! One line of JSON on standard output says how it went (`{"ok": true, ...}` or `{"ok": false, "detail": ...}`);
//! the exit code is 0 only when every step held. The copy is removed at the end.

use std::io::{BufRead, Read, Write};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use rusqlite::{Connection, OpenFlags};
use serde_json::{json, Value};

/// Runs the dry run and gives its verdict; `wait` is how long the started hub has to become well.
pub fn run(source: &Path, scratch: &Path, commit: &str, wait: Duration) -> Value {
    let began = Instant::now();
    let result = trial(source, scratch, commit, wait);
    let _ = std::fs::remove_dir_all(scratch);
    match result {
        Ok(mut found) => {
            found["ok"] = json!(true);
            found["commit"] = json!(commit);
            found["seconds"] = json!(began.elapsed().as_secs());
            found
        }
        Err(detail) => json!({
            "ok": false,
            "commit": commit,
            "seconds": began.elapsed().as_secs(),
            "detail": detail,
        }),
    }
}

fn trial(source: &Path, scratch: &Path, commit: &str, wait: Duration) -> Result<Value, String> {
    let copied = copy(source, scratch)?;
    let mut hub = Hub::start(scratch)?;
    let port = hub.port;
    let deadline = Instant::now() + wait;
    // 3. well, and this commit
    let mut last: String;
    loop {
        if let Some(status) = hub.ended() {
            return Err(format!(
                "the hub ended at its start ({status}): {}",
                hub.said()
            ));
        }
        match get(port, "/healthz") {
            Ok((200, body)) if body["ok"] == json!(true) && body["commit"] == json!(commit) => {
                break
            }
            Ok((status, body)) => last = format!("/healthz answered {status}: {body}"),
            Err(e) => last = e,
        }
        if Instant::now() >= deadline {
            return Err(format!(
                "not well within {} s: {last}; {}",
                wait.as_secs(),
                hub.said()
            ));
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    // a route under /v1/ that reads the database: a share that does not exist is refused, never a server error
    match get(port, &format!("/v1/shares/{}", "A".repeat(22))) {
        Ok((status, _)) if status < 500 => {}
        Ok((status, body)) => return Err(format!("/v1/shares answered {status}: {body}")),
        Err(e) => return Err(format!("/v1/shares: {e}")),
    }
    hub.stop();
    // 4. the migrated copy, read as it was left
    let checked = check(&scratch.join("hub.db"))?;
    Ok(json!({ "copied_bytes": copied, "tables": checked.0, "rows": checked.1 }))
}

/// Step 1: the database by `VACUUM INTO` over a read-only connection (consistent while the live hub writes; the live
/// file is not changed), the hub's keys as they are. Gives the bytes of the copy of the database.
fn copy(source: &Path, scratch: &Path) -> Result<u64, String> {
    let _ = std::fs::remove_dir_all(scratch);
    {
        use std::os::unix::fs::DirBuilderExt;
        std::fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(scratch)
            .map_err(|e| format!("the copy's folder: {e}"))?;
    }
    let live = source.join("hub.db");
    let mut bytes = 0;
    if live.exists() {
        let to = scratch.join("hub.db");
        let vacuum = |from: rusqlite::Result<Connection>| -> Result<(), String> {
            let from = from.map_err(|e| format!("the live database: {e}"))?;
            from.busy_timeout(Duration::from_secs(10))
                .map_err(|e| format!("the live database: {e}"))?;
            from.execute("VACUUM INTO ?1", [to.to_string_lossy()])
                .map(|_| ())
                .map_err(|e| format!("copying the live database: {e}"))
        };
        let flags = OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX;
        let copied = vacuum(Connection::open_with_flags(&live, flags));
        if let Err(first) = copied {
            // A hub that stands closed its database and removed the -wal and -shm files; a read-only folder does
            // not let SQLite make them again. With no hub writing, the file is read as it lies.
            if live.with_extension("db-wal").exists() {
                return Err(first);
            }
            let _ = std::fs::remove_file(&to);
            let mut uri = String::from("file:");
            for b in live.to_string_lossy().bytes() {
                match b {
                    b'%' | b'?' | b'#' => uri.push_str(&format!("%{b:02X}")),
                    _ => uri.push(b as char),
                }
            }
            uri.push_str("?immutable=1");
            vacuum(Connection::open_with_flags(
                uri,
                flags | OpenFlags::SQLITE_OPEN_URI,
            ))
            .map_err(|second| format!("{first}; read as it lies: {second}"))?;
        }
        bytes = std::fs::metadata(&to).map(|m| m.len()).unwrap_or(0);
    }
    for entry in std::fs::read_dir(source)
        .map_err(|e| format!("the live data folder: {e}"))?
        .flatten()
    {
        let name = entry.file_name();
        if name.to_string_lossy().ends_with(".key") && entry.path().is_file() {
            std::fs::copy(entry.path(), scratch.join(&name))
                .map_err(|e| format!("copying {}: {e}", name.to_string_lossy()))?;
        }
    }
    Ok(bytes)
}

/// Step 4: SQLite's own checks, the schema version, and every table read to its end.
fn check(db: &Path) -> Result<(u64, u64), String> {
    let c = Connection::open_with_flags(db, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .map_err(|e| format!("the migrated copy: {e}"))?;
    let fail = |what: &str, e: rusqlite::Error| format!("the migrated copy ({what}): {e}");
    let version: i64 = c
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .map_err(|e| fail("user_version", e))?;
    if version != crate::db::SCHEMA_VERSION {
        return Err(format!(
            "the migrated copy has schema {version}, this hub writes {}",
            crate::db::SCHEMA_VERSION
        ));
    }
    let quick: String = c
        .query_row("PRAGMA quick_check", [], |r| r.get(0))
        .map_err(|e| fail("quick_check", e))?;
    if quick != "ok" {
        return Err(format!("the migrated copy fails quick_check: {quick}"));
    }
    let broken: i64 = c
        .query_row("SELECT count(*) FROM pragma_foreign_key_check", [], |r| {
            r.get(0)
        })
        .map_err(|e| fail("foreign_key_check", e))?;
    if broken > 0 {
        return Err(format!(
            "the migrated copy has {broken} rows whose foreign keys point nowhere"
        ));
    }
    let tables: Vec<String> = c
        .prepare("SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%'")
        .and_then(|mut s| {
            s.query_map([], |r| r.get::<_, String>(0))?
                .collect::<Result<_, _>>()
        })
        .map_err(|e| fail("tables", e))?;
    let mut rows = 0u64;
    for table in &tables {
        let n: i64 = c
            .query_row(
                &format!("SELECT count(*) FROM \"{}\"", table.replace('"', "\"\"")),
                [],
                |r| r.get(0),
            )
            .map_err(|e| fail(table, e))?;
        rows += n as u64;
    }
    Ok((tables.len() as u64, rows))
}

/// Step 2: this program as a hub on the copy.
struct Hub {
    child: Child,
    port: u16,
    log: std::sync::Arc<std::sync::Mutex<String>>,
}

impl Hub {
    fn start(data: &Path) -> Result<Hub, String> {
        let exe = std::env::current_exe().map_err(|e| format!("this program's path: {e}"))?;
        let mut child = Command::new(exe)
            .env("HUB_DATA", data)
            .env("HUB_HOST", "127.0.0.1")
            .env("HUB_PORT", "0")
            .env("HUB_DRY_RUN", "1")
            .env("HUB_ADMIN_PASSWORD_HASH", "")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("starting the hub: {e}"))?;
        // what it says on stderr is kept (the end of it goes into a failure's detail)
        let log = std::sync::Arc::new(std::sync::Mutex::new(String::new()));
        if let Some(mut err) = child.stderr.take() {
            let log = log.clone();
            std::thread::spawn(move || {
                let mut buf = [0u8; 4096];
                while let Ok(n) = err.read(&mut buf) {
                    if n == 0 {
                        break;
                    }
                    let mut held = log.lock().unwrap_or_else(|e| e.into_inner());
                    held.push_str(&String::from_utf8_lossy(&buf[..n]));
                    if held.len() > 64 << 10 {
                        let cut = held.len() - (32 << 10);
                        let cut = (cut..held.len())
                            .find(|i| held.is_char_boundary(*i))
                            .unwrap_or(0);
                        held.drain(..cut);
                    }
                }
            });
        }
        // the first line of stdout names the port (HUB_PORT=0)
        let out = child.stdout.take().ok_or("the hub's stdout")?;
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let mut lines = std::io::BufReader::new(out).lines();
            if let Some(Ok(line)) = lines.next() {
                let _ = tx.send(line);
            }
            // the rest is read and dropped, so the hub never blocks on a full pipe
            for _ in lines {}
        });
        let mut hub = Hub {
            child,
            port: 0,
            log,
        };
        let line = match rx.recv_timeout(Duration::from_secs(60)) {
            Ok(line) => line,
            Err(_) => {
                let said = hub.said();
                hub.stop();
                return Err(format!("the hub named no port: {said}"));
            }
        };
        let port = serde_json::from_str::<Value>(&line)
            .ok()
            .and_then(|v| v["port"].as_u64())
            .and_then(|p| u16::try_from(p).ok());
        match port {
            Some(port) => {
                hub.port = port;
                Ok(hub)
            }
            None => {
                hub.stop();
                Err(format!("the hub's first line is not its port: {line}"))
            }
        }
    }

    fn ended(&mut self) -> Option<std::process::ExitStatus> {
        self.child.try_wait().ok().flatten()
    }

    /// The last line the hub wrote to stderr.
    fn said(&self) -> String {
        let held = self.log.lock().unwrap_or_else(|e| e.into_inner());
        let last = held
            .lines()
            .rev()
            .find(|l| !l.trim().is_empty())
            .unwrap_or("");
        last.chars().take(400).collect()
    }

    fn stop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Drop for Hub {
    fn drop(&mut self) {
        self.stop();
    }
}

/// One GET to the hub on 127.0.0.1: the status and the body as JSON (`null` when it is not JSON).
fn get(port: u16, path: &str) -> Result<(u16, Value), String> {
    let address = std::net::SocketAddr::from(([127, 0, 0, 1], port));
    let mut socket = std::net::TcpStream::connect_timeout(&address, Duration::from_secs(3))
        .map_err(|e| format!("no answer: {e}"))?;
    let _ = socket.set_read_timeout(Some(Duration::from_secs(10)));
    socket
        .write_all(
            format!("GET {path} HTTP/1.1\r\nhost: 127.0.0.1\r\nconnection: close\r\n\r\n")
                .as_bytes(),
        )
        .map_err(|e| format!("asking: {e}"))?;
    let mut answer = Vec::new();
    socket
        .take(1 << 20)
        .read_to_end(&mut answer)
        .map_err(|e| format!("the answer: {e}"))?;
    let text = String::from_utf8_lossy(&answer);
    let status = text
        .split_whitespace()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .ok_or("not an HTTP answer")?;
    let body = text.split_once("\r\n\r\n").map_or("", |b| b.1);
    let json = match (body.find('{'), body.rfind('}')) {
        (Some(a), Some(b)) if a < b => serde_json::from_str(&body[a..=b]).unwrap_or(Value::Null),
        _ => Value::Null,
    };
    Ok((status, json))
}
