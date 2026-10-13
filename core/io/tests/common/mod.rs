//! A hub stand-in for the executor's tests: a TCP listener that reads one HTTP request per connection and lets
//! the test write the answer byte by byte (so it can lie about lengths, stall, or never stop).
#![allow(dead_code)]

use std::future::Future;
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use trommi_client_core::app::{Header, HttpClass, HttpRequest};

pub struct Server {
    pub url: String,
    /// Every request read so far, head and body as they came.
    pub seen: Arc<Mutex<Vec<String>>>,
}

impl Server {
    pub fn count(&self) -> usize {
        self.seen.lock().unwrap().len()
    }
}

/// Reads a request: the head, and the body its Content-Length names.
async fn read_request(stream: &mut TcpStream) -> Option<String> {
    let mut buf = Vec::new();
    let mut byte = [0u8; 1024];
    let head_end = loop {
        if let Some(at) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            break at + 4;
        }
        let n = stream.read(&mut byte).await.ok()?;
        if n == 0 {
            return None;
        }
        buf.extend_from_slice(&byte[..n]);
    };
    let head = String::from_utf8_lossy(&buf[..head_end]).to_lowercase();
    let length: usize = head
        .lines()
        .find_map(|l| l.strip_prefix("content-length: "))
        .and_then(|v| v.trim().parse().ok())
        .unwrap_or(0);
    while buf.len() < head_end + length {
        let n = stream.read(&mut byte).await.ok()?;
        if n == 0 {
            return None;
        }
        buf.extend_from_slice(&byte[..n]);
    }
    Some(String::from_utf8_lossy(&buf).into_owned())
}

/// Listens on a free local port; `answer` gets each connection once its request was read.
pub async fn server<F, Fut>(answer: F) -> Server
where
    F: Fn(TcpStream) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = ()> + Send + 'static,
{
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let seen = Arc::new(Mutex::new(Vec::new()));
    let log = seen.clone();
    let answer = Arc::new(answer);
    tokio::spawn(async move {
        loop {
            let Ok((mut stream, _)) = listener.accept().await else {
                return;
            };
            let (log, answer) = (log.clone(), answer.clone());
            tokio::spawn(async move {
                if let Some(request) = read_request(&mut stream).await {
                    log.lock().unwrap().push(request);
                    answer(stream).await;
                }
            });
        }
    });
    Server { url, seen }
}

/// Writes text and flushes; `false` when the other side is gone.
pub async fn write(stream: &mut TcpStream, text: &[u8]) -> bool {
    stream.write_all(text).await.is_ok() && stream.flush().await.is_ok()
}

/// Whether the client closed the connection within two seconds.
pub async fn closed(stream: &mut TcpStream) -> bool {
    let mut byte = [0u8; 64];
    tokio::time::timeout(std::time::Duration::from_secs(2), async {
        loop {
            match stream.read(&mut byte).await {
                Ok(0) | Err(_) => return,
                Ok(_) => {}
            }
        }
    })
    .await
    .is_ok()
}

pub fn request(method: &str, url: &str) -> HttpRequest {
    HttpRequest {
        method: method.into(),
        url: url.into(),
        headers: Vec::new(),
        body: None,
        timeout_ms: 5000,
        max_answer: HttpRequest::MAX_ANSWER_DEFAULT,
        class: HttpClass::Interactive,
        key: None,
    }
}

pub fn header(name: &str, value: &str) -> Header {
    Header {
        name: name.into(),
        value: value.into(),
    }
}
