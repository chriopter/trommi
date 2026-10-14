//! A misbehaving network between every client and the hub, for the mega test: an HTTP/1.1 proxy on 127.0.0.1 that
//! every client (connector, stand-in, web, iOS) speaks to instead of the hub. The hub runs with `HUB_URL` set to the
//! proxy's address, so links, sign-ins and invites name the proxy and nothing notices it is there.
//!
//! What it does, seeded and switched on and off by the test at any time (`Knobs`):
//! - latency: each request waits up to `latency_ms` before it goes on;
//! - cut streams: a live stream (`text/event-stream`) is cut after a random number of events (`cut`, a chance per
//!   event, which is lost; `cut_after`, a chance per event, right after it went through): the client reconnects
//!   with `Last-Event-ID`;
//! - duplicate events: a stream event is sent twice (`dup`);
//! - down: every request is refused at the socket (`down`), as a hub that is away (the test also restarts the real
//!   hub behind the proxy: requests meanwhile fail the same way).
//!
//! Nothing here reads or changes what is encrypted. What a real hub never does is left out: it sends a stream's
//! events in the order of their change numbers (they are published under the writer's lock, hub/src/app.rs
//! `write_with`) and a page of `/v1/changes` holds each item once, ascending (spec/hub-api.md 19, 20); the clients
//! rely on both (the app core's sync, core/client/src/engine), so swapping events or repeating items would only find the test.
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};

/// The misbehaviour now: probabilities per event or item (0.0 = never).
#[derive(Clone, Copy, Debug, Default)]
pub struct Knobs {
    pub latency_ms: u64,
    pub cut: f64,
    pub cut_after: f64,
    pub dup: f64,
}

impl Knobs {
    pub fn calm() -> Knobs {
        Knobs::default()
    }
    pub fn rough() -> Knobs {
        Knobs {
            latency_ms: 150,
            cut: 0.02,
            cut_after: 0.02,
            dup: 0.08,
        }
    }
}

/// What the proxy did, for the report.
#[derive(Default, Debug, Clone)]
pub struct Counts {
    pub requests: u64,
    pub streams: u64,
    pub refused: u64,
    pub cuts: u64,
    pub dups: u64,
}

struct Shared {
    upstream: Mutex<String>,
    knobs: Mutex<Knobs>,
    down: AtomicBool,
    rng: AtomicU64,
    counts: Mutex<Counts>,
}

impl Shared {
    fn rand(&self) -> f64 {
        // xorshift, one shared state: the order of draws depends on the scheduler, the kinds of faults on the seed
        let mut x = self.rng.load(Ordering::Relaxed);
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.rng.store(x, Ordering::Relaxed);
        (x % 1_000_000) as f64 / 1_000_000.0
    }
    fn knobs(&self) -> Knobs {
        *self.knobs.lock().unwrap()
    }
    fn count(&self, f: impl FnOnce(&mut Counts)) {
        f(&mut self.counts.lock().unwrap())
    }
}

/// The proxy, running until dropped.
pub struct Proxy {
    pub url: String,
    shared: Arc<Shared>,
    task: tokio::task::JoinHandle<()>,
}

impl Drop for Proxy {
    fn drop(&mut self) {
        self.task.abort();
    }
}

impl Proxy {
    /// Binds a free port; requests go to `upstream` once `set_upstream` names it.
    pub async fn start(seed: u64) -> Proxy {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("a port");
        let port = listener.local_addr().unwrap().port();
        let shared = Arc::new(Shared {
            upstream: Mutex::new(String::new()),
            knobs: Mutex::new(Knobs::calm()),
            down: AtomicBool::new(false),
            rng: AtomicU64::new(seed.wrapping_mul(0x2545_F491_4F6C_DD1D) | 1),
            counts: Mutex::new(Counts::default()),
        });
        let s = shared.clone();
        let task = tokio::spawn(async move {
            loop {
                let Ok((conn, _)) = listener.accept().await else { continue };
                let _ = conn.set_nodelay(true);
                let s = s.clone();
                tokio::spawn(async move {
                    let _ = serve(conn, s).await;
                });
            }
        });
        Proxy {
            url: format!("http://127.0.0.1:{port}"),

            shared,
            task,
        }
    }

    /// Where requests go: the hub's own address, `127.0.0.1:<port>`.
    pub fn set_upstream(&self, hub_port: u16) {
        *self.shared.upstream.lock().unwrap() = format!("127.0.0.1:{hub_port}");
    }
    pub fn set(&self, knobs: Knobs) {
        *self.shared.knobs.lock().unwrap() = knobs;
    }
    pub fn knobs(&self) -> Knobs {
        self.shared.knobs()
    }
    pub fn set_down(&self, down: bool) {
        self.shared.down.store(down, Ordering::SeqCst);
    }
    pub fn counts(&self) -> Counts {
        self.shared.counts.lock().unwrap().clone()
    }
}

type Head = (String, Vec<(String, String)>);

/// Reads an HTTP head (start line and fields) up to its empty line; None at the end of the connection.
async fn read_head<R: AsyncBufReadExt + Unpin>(r: &mut R) -> std::io::Result<Option<Head>> {
    let mut start = String::new();
    loop {
        start.clear();
        if r.read_line(&mut start).await? == 0 {
            return Ok(None);
        }
        if !start.trim().is_empty() {
            break;
        }
    }
    let mut fields = Vec::new();
    loop {
        let mut line = String::new();
        if r.read_line(&mut line).await? == 0 {
            return Err(std::io::ErrorKind::UnexpectedEof.into());
        }
        let line = line.trim_end_matches(['\r', '\n']);
        if line.is_empty() {
            break;
        }
        if let Some((k, v)) = line.split_once(':') {
            fields.push((k.trim().to_string(), v.trim().to_string()));
        }
    }
    Ok(Some((start.trim_end().to_string(), fields)))
}

fn field<'a>(fields: &'a [(String, String)], name: &str) -> Option<&'a str> {
    fields
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case(name))
        .map(|(_, v)| v.as_str())
}

fn write_head(start: &str, fields: &[(String, String)]) -> Vec<u8> {
    let mut out = format!("{start}\r\n");
    for (k, v) in fields {
        out.push_str(&format!("{k}: {v}\r\n"));
    }
    out.push_str("\r\n");
    out.into_bytes()
}

/// One chunk of a chunked body; None at its last (empty) chunk, whose trailer is read too.
async fn read_chunk<R: AsyncBufReadExt + Unpin>(r: &mut R) -> std::io::Result<Option<Vec<u8>>> {
    let mut size = String::new();
    if r.read_line(&mut size).await? == 0 {
        return Err(std::io::ErrorKind::UnexpectedEof.into());
    }
    let n = usize::from_str_radix(size.trim().split(';').next().unwrap_or("0"), 16)
        .map_err(|_| std::io::Error::from(std::io::ErrorKind::InvalidData))?;
    if n == 0 {
        // trailer fields, up to an empty line
        loop {
            let mut line = String::new();
            if r.read_line(&mut line).await? == 0 || line.trim().is_empty() {
                break;
            }
        }
        return Ok(None);
    }
    let mut buf = vec![0; n + 2];
    r.read_exact(&mut buf).await?;
    buf.truncate(n);
    Ok(Some(buf))
}

fn chunk(bytes: &[u8]) -> Vec<u8> {
    let mut out = format!("{:x}\r\n", bytes.len()).into_bytes();
    out.extend_from_slice(bytes);
    out.extend_from_slice(b"\r\n");
    out
}

/// One client connection: request after request (keep-alive), each over a fresh connection to the hub.
async fn serve(conn: TcpStream, s: Arc<Shared>) -> std::io::Result<()> {
    let (cr, mut cw) = conn.into_split();
    let mut cr = BufReader::new(cr);
    loop {
        let Some((start, mut fields)) = read_head(&mut cr).await? else {
            return Ok(());
        };
        s.count(|c| c.requests += 1);
                let knobs = s.knobs();
        if knobs.latency_ms > 0 {
            let wait = (s.rand() * knobs.latency_ms as f64) as u64;
            tokio::time::sleep(Duration::from_millis(wait)).await;
        }
        // the request's body, whole (requests are small, or file pieces of a few MB)
        let mut body = Vec::new();
        let chunked_req = field(&fields, "transfer-encoding").is_some_and(|v| v.contains("chunked"));
        if chunked_req {
            while let Some(c) = read_chunk(&mut cr).await? {
                body.extend_from_slice(&c);
            }
            fields.retain(|(k, _)| !k.eq_ignore_ascii_case("transfer-encoding"));
            fields.push(("Content-Length".into(), body.len().to_string()));
        } else if let Some(n) = field(&fields, "content-length").and_then(|v| v.parse::<usize>().ok()) {
            body.resize(n, 0);
            cr.read_exact(&mut body).await?;
        }
        let upstream = s.upstream.lock().unwrap().clone();
        let up = if s.down.load(Ordering::SeqCst) || upstream.is_empty() {
            None
        } else {
            TcpStream::connect(&upstream).await.ok()
        };
        let Some(up) = up else {
            // the hub is away: the connection ends, as a refused one would for a new request
            s.count(|c| c.refused += 1);
            return Ok(());
        };
        let _ = up.set_nodelay(true);
        fields.retain(|(k, _)| !k.eq_ignore_ascii_case("connection"));
        fields.push(("Connection".into(), "close".into()));
        let (ur, mut uw) = up.into_split();
        uw.write_all(&write_head(&start, &fields)).await?;
        uw.write_all(&body).await?;
        uw.flush().await?;
        let mut ur = BufReader::new(ur);
        let Some((status, rfields)) = read_head(&mut ur).await? else {
            return Ok(());
        };
        let sse = field(&rfields, "content-type").is_some_and(|v| v.contains("text/event-stream"));
        let chunked = field(&rfields, "transfer-encoding").is_some_and(|v| v.contains("chunked"));
        let length = field(&rfields, "content-length").and_then(|v| v.parse::<usize>().ok());
        let close_after = !chunked && length.is_none();
        if sse && chunked {
            s.count(|c| c.streams += 1);
            cw.write_all(&write_head(&status, &rfields)).await?;
            cw.flush().await?;
            relay_stream(&mut ur, &mut cw, &s).await?;
            return Ok(());
        }
        if let Some(n) = length {
            let mut rbody = vec![0; n];
            ur.read_exact(&mut rbody).await?;
            cw.write_all(&write_head(&status, &rfields)).await?;
            cw.write_all(&rbody).await?;
        } else if chunked {
            cw.write_all(&write_head(&status, &rfields)).await?;
            while let Some(c) = read_chunk(&mut ur).await? {
                cw.write_all(&chunk(&c)).await?;
            }
            cw.write_all(b"0\r\n\r\n").await?;
        } else {
            cw.write_all(&write_head(&status, &rfields)).await?;
            let mut rest = Vec::new();
            ur.read_to_end(&mut rest).await?;
            cw.write_all(&rest).await?;
        }
        cw.flush().await?;
        if close_after {
            return Ok(());
        }
    }
}

/// The live stream, event by event, with cuts and duplicates.
async fn relay_stream<R, W>(ur: &mut R, cw: &mut W, s: &Shared) -> std::io::Result<()>
where
    R: AsyncBufReadExt + Unpin,
    W: AsyncWriteExt + Unpin,
{
    let mut text: Vec<u8> = Vec::new();
    loop {
        let Some(bytes) = read_chunk(ur).await? else {
            cw.write_all(b"0\r\n\r\n").await?;
            return cw.flush().await;
        };
        text.extend_from_slice(&bytes);
        // whole events: up to a blank line
        while let Some(at) = find_event_end(&text) {
            let event: Vec<u8> = text.drain(..at).collect();
            let knobs = s.knobs();
            let ping = event.windows(10).any(|w| w == b"event: pin") || event.starts_with(b":");
            if !ping && s.rand() < knobs.cut {
                // cut: nothing more on this connection (the event is lost here, read again on reconnect)
                s.count(|c| c.cuts += 1);
                return Ok(());
            }
            let mut out = event.clone();
            if !ping && s.rand() < knobs.dup {
                s.count(|c| c.dups += 1);
                out.extend_from_slice(&event);
            }
            cw.write_all(&chunk(&out)).await?;
            cw.flush().await?;
            if !ping && s.rand() < knobs.cut_after {
                s.count(|c| c.cuts += 1);
                return Ok(());
            }
        }
    }
}

/// The end of the first whole event in `text` (after its blank line), if one is there.
fn find_event_end(text: &[u8]) -> Option<usize> {
    for i in 0..text.len() {
        if text[i] == b'\n' {
            if text.get(i + 1) == Some(&b'\n') {
                return Some(i + 2);
            }
            if text.get(i + 1) == Some(&b'\r') && text.get(i + 2) == Some(&b'\n') {
                return Some(i + 3);
            }
        }
    }
    None
}
