//! How a request is built: the hub's address, the client's name, the token, a path of checked segments.
//!
//! A request is whole when it leaves here ([`HttpRequest`]): the shell adds nothing and follows no redirect. A path
//! is only ever put together from fixed words and ids in base64url (or hex), a query only from numbers, ids and
//! fixed words, a header value only from text checked here: nothing the hub or a person wrote reaches a URL or a
//! header unchecked. Every request names the most its answer may hold (`max_answer`).

use super::json::{bad, Read};
use super::types::Site;
use crate::app::bytes::Bytes;
use crate::app::io::{Header, HttpClass, HttpRequest};
use trommi_core::hub_auth::HubAddress;
use trommi_core::ids::base64url_encode;

/// How long the shell waits for an answer to a JSON route.
pub const TIMEOUT_MS: u32 = 30_000;

/// Base64url of an id or a byte string, as a path segment, a query value or a JSON string.
pub(crate) fn b64(bytes: &[u8]) -> String {
    base64url_encode(bytes)
}

fn plain(text: &str, max: usize, extra: &[u8]) -> bool {
    !text.is_empty()
        && text.len() <= max
        && text
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || extra.contains(&b))
}

pub(crate) struct Build {
    method: &'static str,
    url: String,
    queried: bool,
    headers: Vec<Header>,
    body: Option<Bytes>,
    class: HttpClass,
    max_answer: usize,
    timeout_ms: u32,
    key: Option<String>,
}

/// Starts a request at `site`. `path` is made of fixed words and ids only. `token`: whether the route takes one.
/// Refused when the site is not one a request can go to: an address that is not the canonical one of 12.1.1, a
/// client name or a token of other than plain characters, a route that needs a token and has none.
pub(crate) fn begin(
    site: &Site<'_>,
    method: &'static str,
    path: &str,
    token: bool,
    max_answer: usize,
) -> Read<Build> {
    if HubAddress::parse(site.url).is_err() {
        return bad("the request: the hub's address is not canonical");
    }
    if !plain(site.client, 64, b"._/-") {
        return bad("the request: the client's name is <kind>/<major>.<minor>.<patch>");
    }
    let mut headers = vec![Header {
        name: "trommi-client".to_owned(),
        value: site.client.to_owned(),
    }];
    if token {
        // 16 to 200 of [A-Za-z0-9_-]: nothing else is a token, whoever says so
        match site.token {
            Some(t) if t.len() >= 16 && plain(t, 200, b"_-") => headers.push(Header {
                name: "authorization".to_owned(),
                value: format!("Bearer {t}"),
            }),
            _ => return bad("the request: this route needs a token"),
        }
    }
    Ok(Build {
        method,
        url: format!("{}{}", site.url, path),
        queried: false,
        headers,
        body: None,
        class: HttpClass::Interactive,
        max_answer,
        timeout_ms: TIMEOUT_MS,
        key: None,
    })
}

impl Build {
    /// A query parameter: its value a number, an id in base64url or a fixed word.
    pub(crate) fn query(mut self, name: &'static str, value: impl ToString) -> Self {
        let value = value.to_string();
        debug_assert!(plain(&value, 512, b"_-"));
        self.url.push(if self.queried { '&' } else { '?' });
        self.queried = true;
        self.url.push_str(name);
        self.url.push('=');
        self.url.push_str(&value);
        self
    }

    pub(crate) fn header(mut self, name: &'static str, value: String) -> Self {
        self.headers.push(Header {
            name: name.to_owned(),
            value,
        });
        self
    }

    pub(crate) fn json(mut self, body: &serde_json::Value) -> Self {
        self.body = Some(Bytes(body.to_string().into_bytes()));
        self.header("content-type", "application/json".to_owned())
    }

    /// A body of bytes as they are (a file).
    pub(crate) fn bytes(mut self, body: &[u8]) -> Self {
        self.body = Some(Bytes(body.to_vec()));
        self.header("content-type", "application/octet-stream".to_owned())
    }

    /// A transfer: its class, a longer wait, and for one that may outlive the App its stable key.
    pub(crate) fn transfer(mut self, class: HttpClass, key: Option<String>) -> Self {
        self.class = class;
        self.timeout_ms = super::limits::TRANSFER_TIMEOUT_MS;
        self.key = key;
        self
    }

    pub(crate) fn class(mut self, class: HttpClass) -> Self {
        self.class = class;
        self
    }

    pub(crate) fn done(self) -> HttpRequest {
        HttpRequest {
            method: self.method.to_owned(),
            url: self.url,
            headers: self.headers,
            body: self.body,
            timeout_ms: self.timeout_ms,
            max_answer: u32::try_from(self.max_answer).unwrap_or(u32::MAX),
            class: self.class,
            key: self.key,
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub(crate) const SITE: Site<'static> = Site {
        url: "https://hub.example",
        token: Some("abcdefghijklmnop_-0123"),
        client: "web/1.2.3",
    };

    pub(crate) fn header<'a>(request: &'a HttpRequest, name: &str) -> Option<&'a str> {
        request
            .headers
            .iter()
            .find(|h| h.name == name)
            .map(|h| h.value.as_str())
    }

    #[test]
    fn a_request_is_whole() {
        let r = begin(&SITE, "GET", "/v1/changes", true, 77)
            .unwrap()
            .query("after", 7)
            .query("limit", 200)
            .done();
        assert_eq!(r.method, "GET");
        assert_eq!(r.url, "https://hub.example/v1/changes?after=7&limit=200");
        assert_eq!(header(&r, "trommi-client"), Some("web/1.2.3"));
        assert_eq!(
            header(&r, "authorization"),
            Some("Bearer abcdefghijklmnop_-0123")
        );
        assert_eq!(
            (r.body, r.timeout_ms, r.max_answer, r.key),
            (None, TIMEOUT_MS, 77, None)
        );
        let r = begin(&SITE, "GET", "/v1/x", false, 1).unwrap().done();
        assert_eq!(header(&r, "authorization"), None);
    }

    #[test]
    fn an_address_a_name_or_a_token_that_is_not_plain_makes_no_request() {
        let ok = |site: &Site<'_>| begin(site, "GET", "/v1/x", true, 1).is_ok();
        assert!(ok(&SITE));
        for url in [
            "https://hub.example/",
            "http://hub.example",
            "https://Hub.example",
            "https://hub.example/v1",
            "",
        ] {
            assert!(!ok(&Site { url, ..SITE }), "{url:?}");
        }
        assert!(ok(&Site {
            url: "http://127.0.0.1:8080",
            ..SITE
        }));
        for client in ["", "web 1", "web/1\r\nx: y"] {
            assert!(!ok(&Site { client, ..SITE }), "{client:?}");
        }
        for token in [
            None,
            Some("short"),
            Some("abcdefghijklmnop\n"),
            Some("abcdefghijklmnop q"),
        ] {
            assert!(!ok(&Site { token, ..SITE }), "{token:?}");
        }
        // a route without a token does not look at it
        assert!(begin(
            &Site {
                token: None,
                ..SITE
            },
            "GET",
            "/v1/x",
            false,
            1
        )
        .is_ok());
    }
}
