//! The hub over real HTTP under random requests (README "Fuzzing"), black box: request lines, headers and bodies
//! of every size and form (no body, a JSON object, any bytes, more than the limit; a content-length that lies, a
//! chunked body, two lengths, odd header names and values, invalid UTF-8), on every route, the streaming ones
//! (stream, files, shares) among them, signed in to a real room or not. `FUZZ_HTTP_CASES` (default 300) and
//! `FUZZ_SEED` set the campaign.
//!
//! Properties: the hub never answers 500 (an internal error on a client's input) or anything above 500 but 503,
//! a JSON body above the limit is refused `too-large` before it is read whole, and after every request the hub
//! still answers `/healthz` within five seconds.
mod common;

use common::{found_room, TestHub};
use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::Duration;
use trommi_hub::util::b64;

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n.max(1)
    }
    fn one_in(&mut self, n: u64) -> bool {
        self.below(n) == 0
    }
    fn bytes(&mut self, n: usize) -> Vec<u8> {
        (0..n).map(|_| self.next() as u8).collect()
    }
}

const PATHS: [&str; 28] = [
    "/v1/stream",
    "/v1/files/{id}",
    "/v1/shares/{id}",
    "/v1/rooms",
    "/v1/rooms/{room}/groups",
    "/v1/rooms/{room}/challenge",
    "/v1/rooms/{room}/tokens",
    "/v1/groups",
    "/v1/groups/{id}/commits",
    "/v1/groups/{id}/messages",
    "/v1/groups/{id}/info",
    "/v1/envelopes",
    "/v1/changes",
    "/v1/desk",
    "/v1/key-packages",
    "/v1/key-packages/claim",
    "/v1/invites",
    "/v1/invites/{id}",
    "/v1/account",
    "/v1/account/login",
    "/v1/push",
    "/v1/link",
    "/v1/welcomes",
    "/healthz",
    "/",
    "/v2/x",
    "//v1//rooms",
    "/v1/%2e%2e/%2e%2e/etc/passwd",
];

fn path(rng: &mut Rng, room: &str) -> String {
    let mut p = PATHS[rng.below(PATHS.len() as u64) as usize].replace("{room}", room);
    while p.contains("{id}") {
        let id = match rng.below(4) {
            0 => b64(&rng.bytes(16)),
            1 => b64(&rng.bytes(32)),
            2 => "x".repeat(rng.below(5000) as usize),
            _ => String::from_utf8_lossy(&rng.bytes(8))
                .replace(['/', ' ', '\r', '\n', '?', '#'], "_"),
        };
        p = p.replacen("{id}", &id, 1);
    }
    if rng.one_in(4) {
        p.push_str(&format!(
            "?after={}&limit={}&{}",
            rng.next() as i64,
            rng.below(5000),
            "x=".repeat(rng.below(50) as usize)
        ));
    }
    p
}

fn body(rng: &mut Rng, limit: usize) -> Vec<u8> {
    match rng.below(7) {
        0 => Vec::new(),
        1 => b"{}".to_vec(),
        2 => format!(
            r#"{{"group_info":"{}","sealed_key":"{}","envelope":"{}"}}"#,
            b64(&rng.bytes(200)),
            b64(&rng.bytes(64)),
            b64(&rng.bytes(300))
        )
        .into_bytes(),
        3 => {
            let n = rng.below(4096) as usize;
            rng.bytes(n)
        }
        4 => {
            // just above the limit for JSON: refused before it is read whole
            let mut big = b"{\"x\":\"".to_vec();
            big.resize(limit + 1 + rng.below(1000) as usize, b'a');
            big.extend_from_slice(b"\"}");
            big
        }
        5 => "[".repeat(rng.below(100_000) as usize).into_bytes(),
        _ => format!("{{\"n\":{}}}", "9".repeat(rng.below(20_000) as usize)).into_bytes(),
    }
}

/// Sends one raw request and reads the status line; `None` if the hub closed or said nothing within 20 s.
fn send(port: u16, raw: &[u8]) -> Option<u16> {
    let mut socket = TcpStream::connect(("127.0.0.1", port)).ok()?;
    socket
        .set_read_timeout(Some(Duration::from_secs(20)))
        .ok()?;
    socket
        .set_write_timeout(Some(Duration::from_secs(20)))
        .ok()?;
    // (the write side stays open: hyper drops a connection whose client half-closed it)
    let _ = socket.write_all(raw);
    let mut head = Vec::new();
    let mut byte = [0u8; 1];
    while head.len() < 64 && !head.ends_with(b"\r\n") {
        match socket.read(&mut byte) {
            Ok(1) => head.push(byte[0]),
            _ => break,
        }
    }
    let line = String::from_utf8_lossy(&head);
    line.split(' ').nth(1).and_then(|s| s.trim().parse().ok())
}

fn healthy(hub: &TestHub) -> bool {
    (0..50).any(|_| {
        let ok = send(
            hub.port,
            b"GET /healthz HTTP/1.1\r\nhost: x\r\nconnection: close\r\n\r\n",
        ) == Some(200);
        if !ok {
            std::thread::sleep(Duration::from_millis(100));
        }
        ok
    })
}

#[test]
fn the_hub_survives_random_http_requests() {
    let cases: u64 = std::env::var("FUZZ_HTTP_CASES")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(300);
    let seed: u64 = std::env::var("FUZZ_SEED")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or_else(|| {
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(1, |d| d.as_nanos() as u64)
                | 1
        });
    eprintln!("FUZZ_SEED={seed}");
    let mut rng = Rng(seed);
    // a body that is announced longer than it is waits this long
    let hub = TestHub::start_with(&[
        ("HUB_LIMIT_OPEN_PER_IP_MINUTE", "100000000"),
        ("HUB_BODY_TIMEOUT_MS", "1000"),
    ]);
    let limit = hub.app.cfg.json_limit;
    let (room, _recovery, dev) = found_room(&hub);
    let room = b64(&room);
    let token = dev.headers()[0].1.clone();

    for case in 0..cases {
        let method = [
            "GET", "POST", "PUT", "DELETE", "OPTIONS", "HEAD", "PATCH", "BREW", "G\u{0}T",
        ][rng.below(9) as usize];
        let target = path(&mut rng, &room);
        let body = body(&mut rng, limit);
        let mut raw =
            format!("{method} {target} HTTP/1.1\r\nhost: 127.0.0.1\r\nconnection: close\r\n")
                .into_bytes();
        if !rng.one_in(4) {
            raw.extend_from_slice(
                format!(
                    "authorization: {}\r\n",
                    if rng.one_in(2) {
                        token.clone()
                    } else {
                        format!("Bearer {}", b64(&rng.bytes(32)))
                    }
                )
                .as_bytes(),
            );
        }
        let chunked = rng.one_in(8);
        match rng.below(10) {
            _ if chunked => raw.extend_from_slice(b"transfer-encoding: chunked\r\n"),
            0 => raw.extend_from_slice(
                format!(
                    "content-length: {}\r\n",
                    body.len() + 1 + rng.below(1 << 20) as usize
                )
                .as_bytes(),
            ),
            1 => raw.extend_from_slice(
                format!("content-length: {}\r\ncontent-length: 0\r\n", body.len()).as_bytes(),
            ),
            2 => raw.extend_from_slice(b"content-length: -1\r\n"),
            3 => raw.extend_from_slice(b"content-length: 99999999999999999999999\r\n"),
            _ => raw.extend_from_slice(format!("content-length: {}\r\n", body.len()).as_bytes()),
        }
        for _ in 0..rng.below(4) {
            let header = match rng.below(9) {
                0 => format!(
                    "trommi-client: {}\r\n",
                    ["web/0.0.1", "ios/99.0.0", "", "x/\u{0}"][rng.below(4) as usize]
                ),
                1 => format!("trommi-lease: {}\r\n", rng.next() as i64),
                2 => format!("range: bytes={}-{}\r\n", rng.next(), rng.below(10)),
                3 => format!(
                    "origin: {}\r\n",
                    ["https://app.trommi.com", "null", "https://evil.example"]
                        [rng.below(3) as usize]
                ),
                4 => format!("last-event-id: {}\r\n", rng.next()),
                5 => format!(
                    "cf-connecting-ip: {}\r\n",
                    ["1.2.3.4", "::1", "nonsense", ""][rng.below(4) as usize]
                ),
                6 => format!("x-share-secret: {}\r\n", b64(&rng.bytes(32))),
                7 => format!(
                    "x-{}: {}\r\n",
                    "a".repeat(rng.below(9000) as usize),
                    "b".repeat(rng.below(9000) as usize)
                ),
                _ => {
                    let n = rng.below(60) as usize;
                    String::from_utf8_lossy(&rng.bytes(n)).replace("\r\n", "") + "\r\n"
                }
            };
            raw.extend_from_slice(header.as_bytes());
        }
        raw.extend_from_slice(b"\r\n");
        if chunked {
            raw.extend_from_slice(format!("{:x}\r\n", body.len()).as_bytes());
            raw.extend_from_slice(&body);
            raw.extend_from_slice(if rng.one_in(3) {
                b"\r\nzz\r\n"
            } else {
                b"\r\n0\r\n\r\n"
            });
        } else {
            raw.extend_from_slice(&body);
        }
        let status = send(hub.port, &raw);
        if let Some(status) = status {
            assert!(
                status < 500 || status == 503,
                "case {case} (seed {seed}): {method} {target} answered {status}"
            );
        }
        assert!(
            healthy(&hub),
            "case {case} (seed {seed}): the hub stopped answering after {method} {target}"
        );
    }

    // a JSON body above the limit, announced honestly: refused, as too large
    let mut big = b"{\"x\":\"".to_vec();
    big.resize(limit + 10, b'a');
    big.extend_from_slice(b"\"}");
    let raw = [
        format!("POST /v1/envelopes HTTP/1.1\r\nhost: x\r\nconnection: close\r\nauthorization: {token}\r\ncontent-length: {}\r\n\r\n", big.len()).into_bytes(),
        big,
    ]
    .concat();
    assert_eq!(
        send(hub.port, &raw),
        Some(413),
        "a body above the JSON limit is too large"
    );
}
