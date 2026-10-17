//! Sealed dictation's relay (spec/v1.md 18, hub/src/stt.rs) over HTTP, against a stand-in enclave: the hub passes
//! the sealed body and the EHBP headers on unchanged, swaps the device's token for its Tinfoil key, passes the
//! sealed answer and its nonce back, keeps to its limits, and is off without a key.

mod common;

use common::*;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::sync::{Arc, Mutex};

const KEY: &str = "test-tinfoil-key-not-real";
const ENC: &str = "a1b2c3d4e5f60718293a4b5c6d7e8f90a1b2c3d4e5f60718293a4b5c6d7e8f90";
const NONCE: &str = "00112233445566778899aabbccddeeff00112233445566778899aabbccddeeff";

#[derive(Debug, Clone)]
struct Seen {
    method: String,
    path: String,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

impl Seen {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers.iter().find(|(k, _)| k == name).map(|(_, v)| v.as_str())
    }
}

/// A stand-in enclave on a free port: records each request and answers with the status set in `status`.
struct Enclave {
    origin: String,
    seen: Arc<Mutex<Vec<Seen>>>,
    status: Arc<Mutex<u16>>,
}

impl Enclave {
    fn start() -> Enclave {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let origin = format!("http://127.0.0.1:{}", listener.local_addr().unwrap().port());
        let seen = Arc::new(Mutex::new(Vec::new()));
        let status = Arc::new(Mutex::new(200u16));
        let (s, st) = (seen.clone(), status.clone());
        std::thread::spawn(move || {
            for socket in listener.incoming() {
                let Ok(socket) = socket else { continue };
                let mut reader = BufReader::new(socket.try_clone().unwrap());
                let mut line = String::new();
                if reader.read_line(&mut line).is_err() {
                    continue;
                }
                let mut parts = line.split(' ');
                let method = parts.next().unwrap_or_default().to_string();
                let path = parts.next().unwrap_or_default().to_string();
                let mut headers = vec![];
                loop {
                    let mut l = String::new();
                    reader.read_line(&mut l).unwrap();
                    let l = l.trim_end();
                    if l.is_empty() {
                        break;
                    }
                    if let Some((k, v)) = l.split_once(':') {
                        headers.push((k.trim().to_ascii_lowercase(), v.trim().to_string()));
                    }
                }
                let n: usize = headers
                    .iter()
                    .find(|(k, _)| k == "content-length")
                    .and_then(|(_, v)| v.parse().ok())
                    .unwrap_or(0);
                let mut body = vec![0u8; n];
                reader.read_exact(&mut body).unwrap();
                s.lock().unwrap().push(Seen { method, path: path.clone(), headers, body });
                let code = *st.lock().unwrap();
                let (head, answer): (String, Vec<u8>) = if path == "/attestation" {
                    ("content-type: application/json\r\n".into(), br#"{"domain":"router.inf.tinfoil.sh"}"#.to_vec())
                } else if code == 200 {
                    (format!("ehbp-response-nonce: {NONCE}\r\ncontent-type: application/json\r\n"), b"\x00\x00\x00\x03sealed".to_vec())
                } else {
                    (String::new(), b"no".to_vec())
                };
                let mut out = socket;
                let _ = write!(
                    out,
                    "HTTP/1.1 {code} X\r\n{head}content-length: {}\r\nconnection: close\r\n\r\n",
                    answer.len()
                );
                let _ = out.write_all(&answer);
            }
        });
        Enclave { origin, seen, status }
    }
}

fn hub_with(enclave: &Enclave) -> TestHub {
    TestHub::start_with(&[
        ("HUB_TINFOIL_API_KEY", KEY),
        ("HUB_STT_EXTRA_ORIGINS", &enclave.origin),
        ("HUB_STT_ATTESTATION_URL", &format!("{}/attestation", enclave.origin)),
    ])
}

fn dictate(hub: &TestHub, dev: &Dev, enclave: &str, enc: &str, body: &[u8]) -> Reply {
    dev.raw(
        hub,
        "POST",
        "/v1/stt",
        &[
            ("x-tinfoil-enclave-url", enclave.to_string()),
            ("ehbp-encapsulated-key", enc.to_string()),
            ("content-type", "multipart/form-data; boundary=trommi".to_string()),
            ("trommi-client", "web/1".to_string()),
        ],
        body,
    )
}

#[test]
fn off_without_a_key() {
    let w = World::on(TestHub::start());
    w.ada.get(&w.hub, "/v1/stt/attestation").refused(404, "not-found");
    dictate(&w.hub, &w.ada, "https://router.inf.tinfoil.sh", ENC, b"x").refused(404, "not-found");
}

#[test]
fn relays_sealed_bytes_with_the_hubs_key() {
    let enclave = Enclave::start();
    let w = World::on(hub_with(&enclave));

    // the bundle is passed on as Tinfoil gave it
    let bundle = w.ada.get(&w.hub, "/v1/stt/attestation");
    assert_eq!(bundle.status, 200);
    assert_eq!(bundle.body, br#"{"domain":"router.inf.tinfoil.sh"}"#);

    let sealed: Vec<u8> = (0..70_000u32).map(|i| (i * 7 % 251) as u8).collect();
    let reply = dictate(&w.hub, &w.ada, &enclave.origin, ENC, &sealed);
    assert_eq!(reply.status, 200, "{}", String::from_utf8_lossy(&reply.body));
    assert_eq!(reply.header("ehbp-response-nonce"), Some(NONCE));
    assert_eq!(reply.body, b"\x00\x00\x00\x03sealed");

    let seen = enclave.seen.lock().unwrap().clone();
    let call = seen.iter().find(|s| s.path == "/v1/audio/transcriptions").expect("the enclave was called");
    assert_eq!(call.method, "POST");
    assert_eq!(call.body, sealed, "the sealed body arrives unchanged");
    assert_eq!(call.header("authorization"), Some(&*format!("Bearer {KEY}")));
    assert_eq!(call.header("ehbp-encapsulated-key"), Some(ENC));
    assert_eq!(call.header("content-type"), Some("multipart/form-data; boundary=trommi"));
    // nothing else of the device's request goes on: not its token, not its client
    assert!(call.header("trommi-client").is_none());
    assert!(call.header("x-tinfoil-enclave-url").is_none());
    let token = w.ada.token.clone().unwrap();
    assert!(!call.headers.iter().any(|(_, v)| v.contains(&token)));
}

#[test]
fn refuses_what_it_should() {
    let enclave = Enclave::start();
    let w = World::on(hub_with(&enclave));
    // signed in, or nothing
    request(w.hub.port, "POST", "/v1/stt", &[], b"x").refused(401, "unauthorised");
    request(w.hub.port, "GET", "/v1/stt/attestation", &[], b"").refused(401, "unauthorised");
    // only an enclave under tinfoil.sh, over HTTPS
    for bad in ["https://evil.example", "http://router.tinfoil.sh", "https://tinfoil.sh.evil.example", "https://a@router.tinfoil.sh", "https://router.tinfoil.sh:8443", "https://router.tinfoil.sh/x"] {
        dictate(&w.hub, &w.ada, bad, ENC, b"x").refused(400, "bad-format");
    }
    // the encapsulated key: 32 bytes as hex
    dictate(&w.hub, &w.ada, &enclave.origin, "abc", b"x").refused(400, "bad-format");
    // a body, and not more than the limit
    dictate(&w.hub, &w.ada, &enclave.origin, ENC, b"").refused(400, "bad-format");
    assert!(enclave.seen.lock().unwrap().is_empty(), "nothing refused reached the enclave");
    // the enclave failing or busy: `overloaded`
    *enclave.status.lock().unwrap() = 500;
    dictate(&w.hub, &w.ada, &enclave.origin, ENC, b"x").refused(503, "overloaded");
    *enclave.status.lock().unwrap() = 503;
    dictate(&w.hub, &w.ada, &enclave.origin, ENC, b"x").refused(503, "overloaded");
}

#[test]
fn too_large_is_refused() {
    let enclave = Enclave::start();
    let hub = TestHub::start_with(&[
        ("HUB_TINFOIL_API_KEY", KEY),
        ("HUB_STT_EXTRA_ORIGINS", &enclave.origin),
        ("HUB_LIMIT_STT", "1000"),
    ]);
    let w = World::on(hub);
    dictate(&w.hub, &w.ada, &enclave.origin, ENC, &[7u8; 1001]).refused(413, "too-large");
    assert!(enclave.seen.lock().unwrap().is_empty());
}

#[test]
fn ten_a_minute() {
    let enclave = Enclave::start();
    let w = World::on(hub_with(&enclave));
    for _ in 0..10 {
        assert_eq!(dictate(&w.hub, &w.ada, &enclave.origin, ENC, b"x").status, 200);
    }
    let refused = dictate(&w.hub, &w.ada, &enclave.origin, ENC, b"x");
    refused.refused(429, "rate-limited");
    assert!(refused.header("retry-after").is_some());
    assert_eq!(enclave.seen.lock().unwrap().len(), 10);
}
