//! The HTTP executor against a server that misbehaves: what is sent is the request and nothing more, an answer of
//! any status is an answer, and no answer is read past the request's cap.

mod common;

use common::{closed, header, request, server, write};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use trommi_client_core::app::{IoErrorKind, StreamItem};
use trommi_io::Http;

#[tokio::test]
async fn the_request_goes_out_as_built_and_any_status_is_an_answer() {
    let s = server(|mut stream| async move {
        write(
            &mut stream,
            b"HTTP/1.1 409 Conflict\r\nX-Code: epoch-taken\r\nContent-Length: 5\r\n\r\nhello",
        )
        .await;
    })
    .await;
    let mut r = request("POST", &format!("{}/v1/items?x=1", s.url));
    r.headers = vec![
        header("X-One", "1"),
        header("Content-Type", "application/json"),
    ];
    r.body = Some(b"{\"a\":1}".to_vec().into());
    let answer = Http::new().unwrap().fetch(&r).await.unwrap();
    assert_eq!(answer.status, 409);
    assert_eq!(answer.header("x-code"), Some("epoch-taken"));
    assert_eq!(&answer.body[..], b"hello");
    let seen = s.seen.lock().unwrap()[0].to_lowercase();
    assert!(
        seen.starts_with("post /v1/items?x=1 http/1.1\r\n"),
        "{seen}"
    );
    assert!(
        seen.contains("\r\nx-one: 1\r\n")
            && seen.contains("\r\ncontent-type: application/json\r\n")
    );
    assert!(seen.ends_with("\r\n\r\n{\"a\":1}"));
    for own in [
        "user-agent",
        "cookie",
        "authorization",
        "referer",
        "accept-encoding",
    ] {
        assert!(
            !seen.contains(&format!("\r\n{own}:")),
            "the executor added {own}: {seen}"
        );
    }
}

#[tokio::test]
async fn a_redirect_is_an_answer_and_is_not_followed() {
    let s = server(|mut stream| async move {
        write(
            &mut stream,
            b"HTTP/1.1 302 Found\r\nLocation: /elsewhere\r\nContent-Length: 0\r\n\r\n",
        )
        .await;
    })
    .await;
    let answer = Http::new()
        .unwrap()
        .fetch(&request("GET", &format!("{}/a", s.url)))
        .await
        .unwrap();
    assert_eq!(answer.status, 302);
    assert_eq!(answer.header("location"), Some("/elsewhere"));
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(s.count(), 1);
}

#[tokio::test]
async fn a_length_above_the_cap_fails_before_the_body_is_read() {
    // the head promises a megabyte and the body never comes: only the head can have decided
    let s = server(|mut stream| async move {
        write(
            &mut stream,
            b"HTTP/1.1 200 OK\r\nContent-Length: 1000000\r\n\r\n",
        )
        .await;
        closed(&mut stream).await;
    })
    .await;
    let mut r = request("GET", &s.url);
    r.max_answer = 1000;
    let began = Instant::now();
    let error = Http::new().unwrap().fetch(&r).await.unwrap_err();
    assert_eq!(error.kind, IoErrorKind::TooLarge);
    assert!(
        began.elapsed() < Duration::from_secs(2),
        "it waited for the body"
    );
}

#[tokio::test]
async fn a_body_without_end_is_cut_at_the_cap_and_the_connection_closed() {
    // chunked, no length, and it never stops: the server counts what it could write until the client hangs up
    let written = Arc::new(AtomicUsize::new(0));
    let hung_up = Arc::new(AtomicBool::new(false));
    let (w, h) = (written.clone(), hung_up.clone());
    let s = server(move |mut stream| {
        let (w, h) = (w.clone(), h.clone());
        async move {
            write(
                &mut stream,
                b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n",
            )
            .await;
            let chunk = [b"1000\r\n".as_slice(), &[b'x'; 4096], b"\r\n"].concat();
            while write(&mut stream, &chunk).await {
                w.fetch_add(4096, Ordering::SeqCst);
            }
            h.store(true, Ordering::SeqCst);
        }
    })
    .await;
    let mut r = request("GET", &s.url);
    r.max_answer = 64 << 10;
    r.timeout_ms = 20_000;
    let error = Http::new().unwrap().fetch(&r).await.unwrap_err();
    assert_eq!(error.kind, IoErrorKind::TooLarge);
    assert!(error.detail.len() < 80 && !error.detail.contains("127.0.0.1"));
    for _ in 0..100 {
        if hung_up.load(Ordering::SeqCst) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(hung_up.load(Ordering::SeqCst), "the connection stayed open");
    // what the server got rid of is the cap plus what sockets buffer, never a stream without end
    assert!(
        written.load(Ordering::SeqCst) < 16 << 20,
        "{} bytes were taken",
        written.load(Ordering::SeqCst)
    );
}

#[tokio::test]
async fn exactly_the_cap_passes_and_one_byte_more_does_not() {
    let s = server(|mut stream| async move {
        let body = [
            b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n3e8\r\n".as_slice(),
            &[b'y'; 1000],
            b"\r\n0\r\n\r\n",
        ]
        .concat();
        write(&mut stream, &body).await;
    })
    .await;
    let http = Http::new().unwrap();
    let mut r = request("GET", &s.url);
    r.max_answer = 1000;
    assert_eq!(http.fetch(&r).await.unwrap().body.len(), 1000);
    r.max_answer = 999;
    assert_eq!(
        http.fetch(&r).await.unwrap_err().kind,
        IoErrorKind::TooLarge
    );
    r.max_answer = 0;
    assert_eq!(
        http.fetch(&r).await.unwrap_err().kind,
        IoErrorKind::TooLarge
    );
}

#[tokio::test]
async fn no_answer_in_time_is_a_timeout_and_no_listener_is_offline() {
    let s = server(|mut stream| async move {
        closed(&mut stream).await;
    })
    .await;
    let http = Http::new().unwrap();
    let mut r = request("GET", &s.url);
    r.timeout_ms = 200;
    assert_eq!(http.fetch(&r).await.unwrap_err().kind, IoErrorKind::Timeout);
    // a head in time and a body that stalls is a timeout too: the limit is for the whole answer
    let s = server(|mut stream| async move {
        write(
            &mut stream,
            b"HTTP/1.1 200 OK\r\nContent-Length: 10\r\n\r\nhalf",
        )
        .await;
        closed(&mut stream).await;
    })
    .await;
    let mut r = request("GET", &s.url);
    r.timeout_ms = 300;
    assert_eq!(http.fetch(&r).await.unwrap_err().kind, IoErrorKind::Timeout);
    // a port nobody listens on
    let free = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .unwrap()
        .local_addr()
        .unwrap();
    let r = request("GET", &format!("http://{free}/"));
    assert_eq!(http.fetch(&r).await.unwrap_err().kind, IoErrorKind::Offline);
    // an answer cut in the middle
    let s = server(|mut stream| async move {
        write(
            &mut stream,
            b"HTTP/1.1 200 OK\r\nContent-Length: 10\r\n\r\nhalf",
        )
        .await;
    })
    .await;
    assert_eq!(
        http.fetch(&request("GET", &s.url)).await.unwrap_err().kind,
        IoErrorKind::Offline
    );
    // what is no request at all
    assert_eq!(
        http.fetch(&request("GE T", &s.url)).await.unwrap_err().kind,
        IoErrorKind::Failed
    );
    let mut r = request("GET", &s.url);
    r.headers = vec![header("bad name", "x")];
    assert_eq!(http.fetch(&r).await.unwrap_err().kind, IoErrorKind::Failed);
}

async fn items(http: &Http, r: &trommi_client_core::app::HttpRequest) -> Vec<StreamItem> {
    let got = Arc::new(Mutex::new(Vec::new()));
    let g = got.clone();
    http.stream(r, move |item| {
        g.lock().unwrap().push(item);
        async { true }
    })
    .await;
    let out = got.lock().unwrap().clone();
    out
}

fn text(items: &[StreamItem]) -> String {
    items
        .iter()
        .filter_map(|i| match i {
            StreamItem::Chunk { bytes } => Some(String::from_utf8_lossy(bytes).into_owned()),
            _ => None,
        })
        .collect()
}

#[tokio::test]
async fn a_stream_brings_its_head_its_bytes_and_its_end() {
    let s = server(|mut stream| async move {
        write(&mut stream, b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nTransfer-Encoding: chunked\r\n\r\n").await;
        for piece in ["6\r\ndata: \r\n", "3\r\n1\n\n\r\n", "0\r\n\r\n"] {
            write(&mut stream, piece.as_bytes()).await;
            tokio::time::sleep(Duration::from_millis(30)).await;
        }
    })
    .await;
    let http = Http::new().unwrap();
    let mut r = request("GET", &format!("{}/v1/stream?after=7", s.url));
    r.headers = vec![header("Accept", "text/event-stream")];
    // (an open stream is bounded by the core that parses it, not by the cap of the request)
    r.max_answer = 4;
    let got = items(&http, &r).await;
    assert!(
        matches!(&got[0], StreamItem::Opened { status: 200, headers } if headers.iter().any(|h| h.name == "content-type"))
    );
    assert_eq!(text(&got), "data: 1\n\n");
    assert_eq!(got.last(), Some(&StreamItem::Ended));
    assert!(s.seen.lock().unwrap()[0]
        .to_lowercase()
        .contains("\r\naccept: text/event-stream\r\n"));
}

#[tokio::test]
async fn a_stream_that_is_refused_brings_its_body_within_the_cap() {
    let s = server(|mut stream| async move {
        write(
            &mut stream,
            b"HTTP/1.1 503 Busy\r\nRetry-After: 3\r\nContent-Length: 4\r\n\r\nbusy",
        )
        .await;
    })
    .await;
    let http = Http::new().unwrap();
    let mut r = request("GET", &s.url);
    let got = items(&http, &r).await;
    assert!(matches!(&got[0], StreamItem::Opened { status: 503, .. }));
    assert_eq!(text(&got), "busy");
    assert_eq!(got.last(), Some(&StreamItem::Ended));
    r.max_answer = 3;
    let got = items(&http, &r).await;
    assert_eq!(got.len(), 2);
    assert!(matches!(&got[1], StreamItem::Failed { error } if error.kind == IoErrorKind::TooLarge));
}

#[tokio::test]
async fn a_stream_that_breaks_or_never_opens_fails() {
    let s = server(|mut stream| async move {
        write(
            &mut stream,
            b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n2\r\nab\r\n",
        )
        .await;
    })
    .await;
    let http = Http::new().unwrap();
    let got = items(&http, &request("GET", &s.url)).await;
    assert_eq!(text(&got), "ab");
    assert!(
        matches!(got.last(), Some(StreamItem::Failed { error }) if error.kind == IoErrorKind::Offline)
    );
    let s = server(|mut stream| async move {
        closed(&mut stream).await;
    })
    .await;
    let mut r = request("GET", &s.url);
    r.timeout_ms = 200;
    let got = items(&http, &r).await;
    assert!(
        matches!(&got[..], [StreamItem::Failed { error }] if error.kind == IoErrorKind::Timeout)
    );
}

#[tokio::test]
async fn a_stream_nobody_reads_any_more_stops() {
    let hung_up = Arc::new(AtomicBool::new(false));
    let h = hung_up.clone();
    let s = server(move |mut stream| {
        let h = h.clone();
        async move {
            write(
                &mut stream,
                b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n",
            )
            .await;
            while write(&mut stream, b"1\r\nx\r\n").await {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
            h.store(true, Ordering::SeqCst);
        }
    })
    .await;
    let seen = Arc::new(AtomicUsize::new(0));
    let n = seen.clone();
    Http::new()
        .unwrap()
        .stream(&request("GET", &s.url), move |_| {
            let taken = n.fetch_add(1, Ordering::SeqCst) < 3;
            async move { taken }
        })
        .await;
    assert_eq!(seen.load(Ordering::SeqCst), 4);
    for _ in 0..100 {
        if hung_up.load(Ordering::SeqCst) {
            return;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("the connection stayed open");
}
