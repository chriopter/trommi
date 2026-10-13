//! Files and Share links (spec/v1.md 11): the stored bytes go up and come down as they are; the hub sees
//! ciphertext only.

use super::json::{bad, Json, Read};
use super::limits::{CAP_FILE, CAP_WRITE};
use super::request::{b64, begin};
use super::types::{Answer, ByteRange, FileBytes, Site};
use super::{classify, Failure};
use crate::app::io::{HttpClass, HttpRequest, HttpResponse};
use serde_json::json;
use trommi_core::ids::{FileId, ShareId};

// ---- PUT /v1/files/{file}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct PutAsked {
    pub file: FileId,
    pub size: u64,
}

/// The upload carries a stable key (`put-file/<id>`): a platform may hand it to a transfer that outlives the
/// App, and the same bytes sent twice get the first answer.
pub(crate) fn put_request(
    site: &Site<'_>,
    file: &FileId,
    bytes: &[u8],
) -> Read<(HttpRequest, PutAsked)> {
    if bytes.len() > CAP_FILE {
        return bad("the request: a file of more than 64 MiB");
    }
    let id = b64(file.as_bytes());
    let request = begin(site, "PUT", &format!("/v1/files/{id}"), true, CAP_WRITE)?
        .bytes(bytes)
        .transfer(HttpClass::Upload, Some(format!("put-file/{id}")))
        .done();
    let asked = PutAsked {
        file: *file,
        size: bytes.len() as u64,
    };
    Ok((request, asked))
}

/// The file stored is the one sent: its id and its size. The hash is handed on for the caller to compare.
pub(crate) fn stored(asked: &PutAsked, body: &Json) -> Read<Answer> {
    let o = body.obj("a file answer: an object")?;
    let file = o.need("file_id").id(FileId::from_slice, "file_id")?;
    let size = o.need("size").int("a file's size")?;
    if file != asked.file || size != asked.size {
        return bad("another file than the one sent");
    }
    Ok(Answer::FileStored {
        size,
        sha256: o.need("sha256").fixed("sha256: 32 bytes")?,
    })
}

// ---- GET /v1/files/{file}, GET /v1/shares/{share}

/// How many bytes a range asks for at most.
fn most(range: Option<ByteRange>) -> Read<usize> {
    match range {
        None => Ok(CAP_FILE),
        Some(ByteRange { first, last }) => {
            let last = last.unwrap_or(CAP_FILE as u64 - 1);
            if first > last || last >= CAP_FILE as u64 {
                return bad("the request: a range inside a file of at most 64 MiB");
            }
            Ok((last - first + 1) as usize)
        }
    }
}

fn get(
    site: &Site<'_>,
    path: &str,
    token: bool,
    secret: Option<&[u8; 32]>,
    range: Option<ByteRange>,
) -> Read<HttpRequest> {
    let mut build = begin(site, "GET", path, token, most(range)?)?;
    if let Some(secret) = secret {
        build = build.header("x-share-secret", b64(secret));
    }
    if let Some(ByteRange { first, last }) = range {
        let last = last.map_or(String::new(), |l| l.to_string());
        build = build.header("range", format!("bytes={first}-{last}"));
    }
    Ok(build.transfer(HttpClass::Download, None).done())
}

pub(crate) fn get_request(
    site: &Site<'_>,
    file: &FileId,
    range: Option<ByteRange>,
) -> Read<HttpRequest> {
    get(
        site,
        &format!("/v1/files/{}", b64(file.as_bytes())),
        true,
        None,
        range,
    )
}

pub(crate) fn shared_request(
    site: &Site<'_>,
    share: &ShareId,
    secret: &[u8; 32],
    range: Option<ByteRange>,
) -> Read<HttpRequest> {
    get(
        site,
        &format!("/v1/shares/{}", b64(share.as_bytes())),
        false,
        Some(secret),
        range,
    )
}

/// `bytes a-b/total`, each a decimal number of at most 12 digits.
fn content_range(value: &str) -> Option<(u64, u64, u64)> {
    let number = |text: &str| -> Option<u64> {
        ((1..=12).contains(&text.len()) && text.bytes().all(|b| b.is_ascii_digit()))
            .then(|| text.parse().ok())?
    };
    let (span, total) = value.strip_prefix("bytes ")?.split_once('/')?;
    let (a, b) = span.split_once('-')?;
    Some((number(a)?, number(b)?, number(total)?))
}

/// The bytes of a file answer, checked against what was asked: whole, status 200 and at most 64 MiB; a range,
/// status 206 with a `content-range` that is exactly the range asked for (to the file's end where none was
/// named) and as many bytes as it says.
pub(crate) fn bytes(range: Option<ByteRange>, response: &HttpResponse) -> Result<Answer, Failure> {
    classify(response)?;
    let got = response.body.len() as u64;
    let Some(ByteRange { first, last }) = range else {
        if response.status != 200 || got > CAP_FILE as u64 {
            return bad("not the file asked for");
        }
        return Ok(Answer::File(FileBytes {
            bytes: response.body.to_vec(),
            first: 0,
            size: got,
        }));
    };
    let named = response.header("content-range").and_then(content_range);
    let (Some((a, b, total)), 206) = (named, response.status) else {
        return bad("not the range asked for");
    };
    let fits = total >= 1
        && total <= CAP_FILE as u64
        && a == first
        && a <= b
        && b < total
        && b == last.map_or(total - 1, |last| last.min(total - 1))
        && got == b - a + 1;
    if !fits {
        return bad("not the range asked for");
    }
    Ok(Answer::File(FileBytes {
        bytes: response.body.to_vec(),
        first,
        size: total,
    }))
}

// ---- deletions, POST /v1/shares

pub(crate) fn delete_file_request(site: &Site<'_>, file: &FileId) -> Read<HttpRequest> {
    Ok(begin(
        site,
        "DELETE",
        &format!("/v1/files/{}", b64(file.as_bytes())),
        true,
        CAP_WRITE,
    )?
    .done())
}

pub(crate) fn delete_share_request(site: &Site<'_>, share: &ShareId) -> Read<HttpRequest> {
    Ok(begin(
        site,
        "DELETE",
        &format!("/v1/shares/{}", b64(share.as_bytes())),
        true,
        CAP_WRITE,
    )?
    .done())
}

pub(crate) fn deleted(body: &Json) -> Read<Answer> {
    match body.obj("a deletion: an object")?.need("deleted") {
        Json::Bool(true) => Ok(Answer::Done),
        _ => bad("deleted: true"),
    }
}

pub(crate) fn share_request(
    site: &Site<'_>,
    share: &ShareId,
    secret_hash: &[u8; 32],
    file: &FileId,
    expires_at: u64,
) -> Read<HttpRequest> {
    if expires_at > super::json::MAX_NUMBER {
        return bad("the request: a time beyond any number of the protocol");
    }
    let body = json!({
        "share_id": b64(share.as_bytes()),
        "secret_hash": b64(secret_hash),
        "file_id": b64(file.as_bytes()),
        "expires_at": expires_at,
    });
    Ok(begin(site, "POST", "/v1/shares", true, CAP_WRITE)?
        .json(&body)
        .done())
}

/// The link registered is the one asked for.
pub(crate) fn shared(share: &ShareId, body: &Json) -> Read<Answer> {
    let o = body.obj("a share: an object")?;
    if o.need("share_id").id(ShareId::from_slice, "share_id")? != *share {
        return bad("another share than the one registered");
    }
    Ok(Answer::Shared {
        expires_at: o.need("expires_at").int("a share's expires_at")?,
    })
}
