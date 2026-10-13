//! `Effect::Http` and `Effect::OpenStream` over reqwest: exactly the request the core built, and its answer whole
//! or not at all.
//!
//! The executor adds nothing and follows nothing: no header, no cookie, no redirect (a 3xx is an answer), no
//! retry. A body is read up to the request's `max_answer`; the first byte beyond it, or a `Content-Length` above
//! it, ends the request as `too_large` and closes the connection. So at most `max_answer` bytes plus the
//! transport's own last chunk are ever held for one request.

use std::future::Future;
use std::time::Duration;
use trommi_client_core::app::{
    Header, HttpRequest, HttpResponse, IoError, IoErrorKind, StreamItem,
};

/// The HTTP client of one executor.
#[derive(Clone)]
pub struct Http {
    client: reqwest::Client,
}

fn error(kind: IoErrorKind, detail: &str) -> IoError {
    IoError {
        kind,
        detail: detail.to_string(),
    }
}

/// Why reqwest gave no answer, as the contract sorts it. The detail is a word of ours, never the error's text
/// (which names the URL).
fn no_answer(e: reqwest::Error) -> IoError {
    if e.is_builder() {
        error(IoErrorKind::Failed, "the request is not one")
    } else if e.is_timeout() {
        error(IoErrorKind::Timeout, "timed out")
    } else if e.is_connect() {
        error(IoErrorKind::Offline, "no connection")
    } else {
        error(IoErrorKind::Offline, "the connection broke")
    }
}

fn too_large() -> IoError {
    error(
        IoErrorKind::TooLarge,
        "the answer is longer than the request allows",
    )
}

fn headers_of(response: &reqwest::Response) -> Vec<Header> {
    response
        .headers()
        .iter()
        .filter_map(|(name, value)| {
            Some(Header {
                name: name.as_str().to_string(),
                value: value.to_str().ok()?.to_string(),
            })
        })
        .collect()
}

impl Http {
    pub fn new() -> Result<Self, String> {
        reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .referer(false)
            .connect_timeout(Duration::from_secs(30))
            .build()
            .map(|client| Self { client })
            .map_err(|_| "no HTTP client".to_string())
    }

    /// Sends the request and waits for the answer's head.
    async fn head(&self, request: &HttpRequest) -> Result<reqwest::Response, IoError> {
        let method = reqwest::Method::from_bytes(request.method.as_bytes())
            .map_err(|_| error(IoErrorKind::Failed, "the method is not one"))?;
        let mut builder = self.client.request(method, &request.url);
        for h in &request.headers {
            builder = builder.header(&h.name, &h.value);
        }
        if let Some(body) = &request.body {
            builder = builder.body(body.0.clone());
        }
        builder.send().await.map_err(no_answer)
    }

    /// One request, one answer: any status is an answer. The whole of it, head and body, within `timeout_ms`.
    pub async fn fetch(&self, request: &HttpRequest) -> Result<HttpResponse, IoError> {
        let whole = async {
            let mut response = self.head(request).await?;
            let status = response.status().as_u16();
            let headers = headers_of(&response);
            let body = body(&mut response, request.max_answer).await?;
            Ok(HttpResponse {
                status,
                headers,
                body: body.into(),
            })
        };
        match tokio::time::timeout(Duration::from_millis(request.timeout_ms.into()), whole).await {
            Ok(result) => result,
            Err(_) => Err(error(IoErrorKind::Timeout, "timed out")),
        }
    }

    /// A streamed GET: `opened`, then the bytes as they come, then `ended` or `failed`. `emit` hands each item on
    /// and waits until it was taken (a slow reader slows the stream, nothing piles up); it answers `false` when
    /// nobody listens any more. `timeout_ms` bounds the wait for the head; an open stream has no time limit. An
    /// answer that is no 200 is read as a body, bounded by `max_answer`.
    pub async fn stream<F, Fut>(&self, request: &HttpRequest, mut emit: F)
    where
        F: FnMut(StreamItem) -> Fut,
        Fut: Future<Output = bool>,
    {
        let head = tokio::time::timeout(
            Duration::from_millis(request.timeout_ms.into()),
            self.head(request),
        )
        .await
        .unwrap_or_else(|_| Err(error(IoErrorKind::Timeout, "timed out")));
        let mut response = match head {
            Ok(response) => response,
            Err(error) => {
                emit(StreamItem::Failed { error }).await;
                return;
            }
        };
        let status = response.status().as_u16();
        let opened = StreamItem::Opened {
            status,
            headers: headers_of(&response),
        };
        if !emit(opened).await {
            return;
        }
        let last = if status == 200 {
            loop {
                match response.chunk().await {
                    Ok(Some(chunk)) => {
                        let bytes = chunk.to_vec().into();
                        if !emit(StreamItem::Chunk { bytes }).await {
                            return;
                        }
                    }
                    Ok(None) => break StreamItem::Ended,
                    Err(e) => {
                        break StreamItem::Failed {
                            error: no_answer(e),
                        }
                    }
                }
            }
        } else {
            match body(&mut response, request.max_answer).await {
                Ok(bytes) => {
                    if !bytes.is_empty()
                        && !emit(StreamItem::Chunk {
                            bytes: bytes.into(),
                        })
                        .await
                    {
                        return;
                    }
                    StreamItem::Ended
                }
                Err(error) => StreamItem::Failed { error },
            }
        };
        emit(last).await;
    }
}

/// The body, at most `cap` bytes of it: a length announced above the cap fails before a byte is read, a body
/// that runs past it at the chunk that does. Returning drops the response, which closes its connection.
async fn body(response: &mut reqwest::Response, cap: u32) -> Result<Vec<u8>, IoError> {
    let cap = cap as usize;
    if response.content_length().is_some_and(|n| n > cap as u64) {
        return Err(too_large());
    }
    let mut out = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(no_answer)? {
        if chunk.len() > cap - out.len() {
            return Err(too_large());
        }
        out.extend_from_slice(&chunk);
    }
    Ok(out)
}
