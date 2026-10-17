//! Sealed dictation (spec/v1.md 18): Tinfoil's attestation bundle through the hub, and a recording sealed to the
//! attested enclave (EHBP) relayed by the hub. The hub reads neither: the bundle is checked on the device, and
//! the body and the answer are ciphertext. Every header value is checked here before it leaves.

use super::json::{bad, Read};
use super::request::begin;
use super::types::{Answer, Site};
use super::{classify, Failure};
use crate::app::io::{HttpClass, HttpRequest, HttpResponse};

/// The most an attestation bundle may hold.
pub const CAP_BUNDLE: usize = 4 << 20;
/// The most a sealed transcript may hold.
pub const CAP_TRANSCRIPT: usize = 1 << 20;
/// How long the person may wait for a transcript of five minutes.
const STT_TIMEOUT_MS: u32 = 180_000;

/// A recording sealed to an enclave: what `POST /v1/stt` carries. `Debug` shows the size only.
#[derive(Clone, PartialEq, Eq)]
pub struct SttSealed {
    /// `https://<host>` of the enclave whose key sealed it: a host under `tinfoil.sh`.
    pub enclave: String,
    /// `Ehbp-Encapsulated-Key`, 64 lowercase hex digits.
    pub encapsulated_key: String,
    /// The form's type: `multipart/form-data; boundary=<plain>`.
    pub content_type: String,
    /// The model, also in the sealed form: the enclave refuses a body that names another (`X-Tinfoil-Model`).
    pub model: String,
    pub body: Vec<u8>,
}

impl std::fmt::Debug for SttSealed {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "SttSealed({} bytes)", self.body.len())
    }
}

/// An enclave's address as the relay takes it: `https://` and a host under `tinfoil.sh`, nothing else.
pub fn enclave_ok(url: &str) -> bool {
    let Some(host) = url.strip_prefix("https://") else { return false };
    !host.is_empty()
        && host.len() <= 253
        && host.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'.')
        && !host.contains("..")
        && !host.starts_with('.')
        && (host == "tinfoil.sh" || host.ends_with(".tinfoil.sh"))
}

pub(crate) fn attestation_request(site: &Site<'_>) -> Read<HttpRequest> {
    Ok(begin(site, "GET", "/v1/stt/attestation", true, CAP_BUNDLE)?.done())
}

pub(crate) fn sealed_request(site: &Site<'_>, sealed: &SttSealed) -> Read<HttpRequest> {
    let hexed = sealed.encapsulated_key.len() == 64 && sealed.encapsulated_key.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b));
    let boundary = sealed.content_type.strip_prefix("multipart/form-data; boundary=").unwrap_or_default();
    let plain = |t: &str, max: usize| !t.is_empty() && t.len() <= max && t.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'.' || b == b'_');
    if !enclave_ok(&sealed.enclave) || !hexed || !plain(boundary, 70) || !plain(&sealed.model, 64) {
        return bad("the request: a sealed dictation's headers are not plain");
    }
    if sealed.body.is_empty() || sealed.body.len() > super::limits::CAP_FILE {
        return bad("the request: a sealed dictation's size");
    }
    let mut request = begin(site, "POST", "/v1/stt", true, CAP_TRANSCRIPT)?
        .header("x-tinfoil-enclave-url", sealed.enclave.clone())
        .header("ehbp-encapsulated-key", sealed.encapsulated_key.clone())
        .header("x-tinfoil-model", sealed.model.clone())
        .bytes(&sealed.body)
        .class(HttpClass::Interactive)
        .done();
    // (the form's type, not the bytes' default)
    request.headers.retain(|h| h.name != "content-type");
    request.headers.push(crate::app::io::Header { name: "content-type".into(), value: sealed.content_type.clone() });
    request.timeout_ms = STT_TIMEOUT_MS;
    Ok(request)
}

/// The bundle's bytes as they came (the device checks them, not the codec): a JSON object of at most 4 MiB.
pub(crate) fn bundle(response: &HttpResponse) -> Result<Answer, Failure> {
    classify(response)?;
    if response.status != 200 || response.body.len() > CAP_BUNDLE || response.body.first() != Some(&b'{') {
        return bad("an attestation bundle: a JSON object");
    }
    Ok(Answer::SttBundle(response.body.to_vec()))
}

/// The sealed transcript and its `Ehbp-Response-Nonce` (64 hex digits).
pub(crate) fn transcript(response: &HttpResponse) -> Result<Answer, Failure> {
    classify(response)?;
    let nonce = response.header("ehbp-response-nonce").filter(|n| n.len() == 64 && n.bytes().all(|b| b.is_ascii_hexdigit()));
    match nonce {
        Some(nonce) if response.status == 200 && response.body.len() <= CAP_TRANSCRIPT => {
            Ok(Answer::SttSealed { nonce: nonce.to_ascii_lowercase(), body: response.body.to_vec() })
        }
        _ => bad("a sealed transcript: status 200 and its nonce"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hub::request::tests::{header, SITE};

    fn sealed() -> SttSealed {
        SttSealed {
            enclave: "https://router.inf6.tinfoil.sh".into(),
            encapsulated_key: "ab".repeat(32),
            content_type: "multipart/form-data; boundary=trommi-1".into(),
            model: "voxtral-small-24b".into(),
            body: vec![1, 2, 3],
        }
    }

    #[test]
    fn the_relay_request_carries_only_checked_headers() {
        let r = sealed_request(&SITE, &sealed()).unwrap();
        assert_eq!((r.method.as_str(), r.url.as_str()), ("POST", "https://hub.example/v1/stt"));
        assert_eq!(header(&r, "x-tinfoil-enclave-url"), Some("https://router.inf6.tinfoil.sh"));
        assert_eq!(header(&r, "ehbp-encapsulated-key"), Some(&*"ab".repeat(32)));
        assert_eq!(header(&r, "content-type"), Some("multipart/form-data; boundary=trommi-1"));
        assert_eq!(r.headers.iter().filter(|h| h.name == "content-type").count(), 1);
        assert_eq!(r.max_answer as usize, CAP_TRANSCRIPT);
        for bad in [
            SttSealed { enclave: "https://evil.example".into(), ..sealed() },
            SttSealed { enclave: "http://router.tinfoil.sh".into(), ..sealed() },
            SttSealed { enclave: "https://router.tinfoil.sh\r\nx: y".into(), ..sealed() },
            SttSealed { encapsulated_key: "AB".repeat(32), ..sealed() },
            SttSealed { content_type: "text/plain".into(), ..sealed() },
            SttSealed { model: "a b".into(), ..sealed() },
            SttSealed { body: vec![], ..sealed() },
        ] {
            assert!(sealed_request(&SITE, &bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn a_transcript_needs_its_nonce() {
        let ok = HttpResponse { status: 200, headers: vec![crate::app::io::Header { name: "Ehbp-Response-Nonce".into(), value: "AB".repeat(32) }], body: crate::app::bytes::Bytes(vec![0, 0, 0, 1, 9]) };
        assert!(matches!(transcript(&ok), Ok(Answer::SttSealed { ref nonce, .. }) if *nonce == "ab".repeat(32)));
        let no_nonce = HttpResponse { headers: vec![], ..ok.clone() };
        assert!(transcript(&no_nonce).is_err());
        let refused = HttpResponse { status: 503, headers: vec![], body: crate::app::bytes::Bytes(br#"{"error":"overloaded","message":"x"}"#.to_vec()) };
        assert!(matches!(transcript(&refused), Err(Failure::NotNow { status: 503, .. })));
    }
}
