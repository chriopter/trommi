//! The chat bridge's client: one turn of a chat agent through its own OpenAI-compatible chat-completions server on
//! this computer (Hermes Agent's API server mode). The connector sends what came from the board as the turn's user
//! message and reads the answer as it streams: its words, the tools it runs, and the approvals it asks for. The
//! conversation lives in the agent: the request names it (`X-Hermes-Session-Id`), and only the new message is sent.
//!
//! What goes over this connection is plaintext, so it goes only to this computer (loopback) unless the person
//! configured an https address of their own ([`Endpoint::check`]); `localhost` is taken as 127.0.0.1, never looked up.
//! The key and the address stay in a file beside the device keys on this computer ([`Endpoint::file`]); nothing of
//! this reaches the hub.
use futures_util::StreamExt;
use serde_json::{json, Value};

/// Where the agent's server listens, and how the connector signs in to it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Endpoint {
    /// The server's base address, without `/v1` ("http://127.0.0.1:8642").
    pub url: String,
    pub key: String,
    pub model: String,
}

/// What one turn hands back while it streams.
#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    /// The server took the turn (it answered 200): the message is the agent's now.
    Started,
    /// A piece of the answer's words.
    Text(String),
    /// A tool started (`running`) or ended (`completed`): its name and its label as the agent shows it.
    Tool { id: String, tool: String, label: String, done: bool },
    /// The agent asks to run something that needs the human's yes (Hermes `approval.request`).
    Approval(Approval),
    /// The turn ended: `stop`, `length`, `error`, `cancelled`, with the agent's error text if any.
    Done { finish: String, error: Option<String> },
}

#[derive(Clone, Debug, PartialEq)]
pub struct Approval {
    /// The run that waits (the completion's id), and the request within it.
    pub run_id: String,
    pub request_id: Option<String>,
    pub command: String,
    pub description: String,
}

/// The longest a turn may take as a whole, and the longest the server may stay silent (it sends a keep-alive every
/// ten seconds while the agent works).
pub const TURN_MAX_MS: u64 = 30 * 60 * 1000;
pub const SILENCE_MAX_MS: u64 = 120_000;

impl Endpoint {
    /// Where a preset's endpoint is kept: beside the device keys, readable by this user only.
    pub fn file(keys_dir: &std::path::Path, preset: &str) -> std::path::PathBuf {
        keys_dir.join("agents").join(format!("{}.json", preset.chars().filter(char::is_ascii_alphanumeric).collect::<String>()))
    }

    /// The endpoint `setup` kept for a preset (none, or one that fails [`Endpoint::check`]: none).
    pub fn load(keys_dir: &std::path::Path, preset: &str) -> Option<Endpoint> {
        let file = Self::file(keys_dir, preset);
        // (a link is not followed: the file is the connector's own)
        if std::fs::symlink_metadata(&file).ok()?.file_type().is_symlink() {
            return None;
        }
        let v = crate::slots::read_json(&file)?;
        let s = |k: &str| v.get(k).and_then(Value::as_str).unwrap_or("").to_string();
        let e = Endpoint { url: s("url"), key: s("key"), model: s("model") };
        (e.check().is_ok() && !e.key.is_empty()).then_some(e)
    }

    /// Keeps the endpoint: a new file written whole (0600) and moved in place, in a folder only this user opens.
    pub fn save(&self, keys_dir: &std::path::Path, preset: &str) -> std::io::Result<std::path::PathBuf> {
        use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
        let file = Self::file(keys_dir, preset);
        let dir = file.parent().unwrap_or(keys_dir);
        std::fs::DirBuilder::new().recursive(true).mode(0o700).create(dir)?;
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))?;
        let tmp = dir.join(format!(".{}.{}", crate::util::random_hex(6), std::process::id()));
        {
            use std::io::Write;
            use std::os::unix::fs::OpenOptionsExt;
            let mut f = std::fs::OpenOptions::new().write(true).create_new(true).mode(0o600).open(&tmp)?;
            f.write_all(json!({ "url": self.url, "key": self.key, "model": self.model }).to_string().as_bytes())?;
            f.sync_all()?;
        }
        std::fs::rename(&tmp, &file).inspect_err(|_| {
            let _ = std::fs::remove_file(&tmp);
        })?;
        Ok(file)
    }

    /// Whether the address may carry plaintext: http only to this computer (127.0.0.0/8, ::1, localhost); any other
    /// host only over https. Nothing else (no user info, no other scheme).
    pub fn check(&self) -> Result<(), String> {
        let u = reqwest::Url::parse(&self.url).map_err(|e| format!("not an address: {e}"))?;
        if !u.username().is_empty() || u.password().is_some() {
            return Err("the address may not hold a user or password".into());
        }
        let host = u.host_str().unwrap_or("");
        let local = host == "localhost"
            || host.trim_start_matches('[').trim_end_matches(']').parse::<std::net::IpAddr>().is_ok_and(|ip| ip.is_loopback());
        match u.scheme() {
            "http" if local => Ok(()),
            "https" if !host.is_empty() => Ok(()),
            "http" => Err(format!("plain http goes only to this computer (127.0.0.1, ::1, localhost), not to {host}: use https")),
            s => Err(format!("unknown scheme {s}")),
        }
    }

    fn base(&self) -> String {
        let mut base = self.url.trim_end_matches('/').trim_end_matches("/v1").to_string();
        if let Ok(mut u) = reqwest::Url::parse(&base) {
            if u.host_str() == Some("localhost") && u.set_host(Some("127.0.0.1")).is_ok() {
                base = u.as_str().trim_end_matches('/').to_string();
            }
        }
        base
    }

    /// Starts one turn: `text` as the user's message in the conversation `conversation`. The answer streams into
    /// `on`; returns once the turn ended (or failed: then as `Done { finish: "error" }`). Dropping the future ends the
    /// request, and the server interrupts the agent (Hermes: a closed stream is a stop).
    pub async fn turn(&self, conversation: &str, text: &str, mut on: impl FnMut(Event)) {
        let done = |e: String| Event::Done { finish: "error".into(), error: Some(e) };
        if let Err(e) = self.check() {
            return on(done(e));
        }
        let client = match client() {
            Ok(c) => c,
            Err(e) => return on(done(e.to_string())),
        };
        let body = json!({ "model": self.model, "stream": true, "messages": [{ "role": "user", "content": text }] });
        let sent = client
            .post(format!("{}/v1/chat/completions", self.base()))
            .bearer_auth(&self.key)
            .header("X-Hermes-Session-Id", conversation)
            .header("Accept", "text/event-stream")
            .header("Content-Type", "application/json")
            .body(body.to_string())
            .send();
        let resp = match tokio::time::timeout(std::time::Duration::from_millis(SILENCE_MAX_MS), sent).await {
            Ok(Ok(r)) => r,
            Ok(Err(e)) => return on(done(format!("the agent's server did not answer: {}", without_url(&e)))),
            Err(_) => return on(done("the agent's server did not answer in time".into())),
        };
        if !resp.status().is_success() {
            let status = resp.status();
            let said = resp.text().await.unwrap_or_default();
            let why = serde_json::from_str::<Value>(&said).ok().and_then(|v| v["error"]["message"].as_str().map(str::to_string)).unwrap_or_default();
            return on(done(format!("the agent's server answered {status}{}", if why.is_empty() { String::new() } else { format!(": {}", cut(&why, 300)) })));
        }
        on(Event::Started);
        let mut parser = Sse::default();
        let mut stream = resp.bytes_stream();
        let started = std::time::Instant::now();
        let mut ended = false;
        loop {
            let left = TURN_MAX_MS.saturating_sub(started.elapsed().as_millis() as u64).min(SILENCE_MAX_MS);
            let chunk = match tokio::time::timeout(std::time::Duration::from_millis(left.max(1)), stream.next()).await {
                Ok(Some(Ok(c))) => c,
                Ok(Some(Err(e))) => {
                    on(done(format!("the stream broke off: {}", without_url(&e))));
                    return;
                }
                Ok(None) => break,
                Err(_) => {
                    on(done("the agent's server went silent".into()));
                    return;
                }
            };
            for (name, data) in parser.push(&chunk) {
                for e in events(name.as_deref(), &data) {
                    ended |= matches!(e, Event::Done { .. });
                    on(e);
                }
            }
        }
        if !ended {
            on(done("the stream ended without its last part".into()));
        }
    }

    /// The human's answer to an approval: `once` or `deny`.
    pub async fn approve(&self, a: &Approval, yes: bool) -> Result<(), String> {
        self.check()?;
        let mut body = json!({ "choice": if yes { "once" } else { "deny" } });
        if let Some(r) = &a.request_id {
            body["request_id"] = json!(r);
        }
        let run: String = a.run_id.chars().filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_').collect();
        let resp = client()
            .map_err(|e| e.to_string())?
            .post(format!("{}/v1/runs/{run}/approval", self.base()))
            .bearer_auth(&self.key)
            .header("Content-Type", "application/json")
            .body(body.to_string())
            .timeout(std::time::Duration::from_secs(30))
            .send()
            .await
            .map_err(|e| without_url(&e))?;
        if resp.status().is_success() {
            Ok(())
        } else {
            Err(format!("the agent's server answered {}", resp.status()))
        }
    }
}

/// The HTTP client: no redirect (the words go to this address or nowhere), no proxy from the environment (a proxy
/// would carry them off this computer).
fn client() -> reqwest::Result<reqwest::Client> {
    reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .no_proxy()
        .connect_timeout(std::time::Duration::from_secs(10))
        .build()
}

/// An error without the request's address (it holds nothing secret, but keeps lines short).
fn without_url(e: &reqwest::Error) -> String {
    let mut s = e.to_string();
    if let Some(u) = e.url() {
        s = s.replace(u.as_str(), "");
    }
    s.trim().trim_end_matches(" ()").to_string()
}

fn cut(s: &str, max: usize) -> String {
    s.chars().take(max).collect()
}

/// What one server-sent event says.
pub fn events(name: Option<&str>, data: &str) -> Vec<Event> {
    if data.trim() == "[DONE]" {
        return vec![];
    }
    let Ok(v) = serde_json::from_str::<Value>(data) else { return vec![] };
    let s = |v: &Value| v.as_str().unwrap_or("").to_string();
    match name {
        Some("hermes.tool.progress") => {
            let label = [s(&v["emoji"]), s(&v["label"])].into_iter().filter(|x| !x.is_empty()).collect::<Vec<_>>().join(" ");
            let label = if label.is_empty() { s(&v["tool"]) } else { label };
            vec![Event::Tool { id: s(&v["toolCallId"]), tool: cut(&s(&v["tool"]), 80), label: cut(&label, 200), done: v["status"] == "completed" }]
        }
        Some("approval.request") => vec![Event::Approval(Approval {
            run_id: s(&v["run_id"]),
            request_id: v["request_id"].as_str().map(str::to_string),
            command: cut(&s(&v["command"]), 2000),
            description: cut(&s(&v["description"]), 500),
        })],
        Some(_) => vec![],
        None => {
            let mut out = vec![];
            let choice = &v["choices"][0];
            if let Some(t) = choice["delta"]["content"].as_str().filter(|t| !t.is_empty()) {
                out.push(Event::Text(t.to_string()));
            }
            if let Some(f) = choice["finish_reason"].as_str() {
                let error = v["error"]["message"].as_str().map(|m| cut(m, 300));
                out.push(Event::Done { finish: f.to_string(), error });
            }
            out
        }
    }
}

/// A server-sent events reader: bytes in, (event name, data) out.
#[derive(Default)]
pub struct Sse {
    buf: Vec<u8>,
    name: Option<String>,
    data: Vec<String>,
}

/// The most an event may weigh before it is dropped (a picture as a data: address can be large).
const EVENT_MAX: usize = 16 * 1024 * 1024;

impl Sse {
    pub fn push(&mut self, bytes: &[u8]) -> Vec<(Option<String>, String)> {
        self.buf.extend_from_slice(bytes);
        let mut out = vec![];
        while let Some(i) = self.buf.iter().position(|b| *b == b'\n') {
            let line: Vec<u8> = self.buf.drain(..=i).collect();
            let line = String::from_utf8_lossy(&line);
            let line = line.trim_end_matches(['\n', '\r']);
            if line.is_empty() {
                if !self.data.is_empty() {
                    out.push((self.name.take(), self.data.join("\n")));
                }
                self.name = None;
                self.data.clear();
            } else if let Some(v) = line.strip_prefix("data:") {
                self.data.push(v.strip_prefix(' ').unwrap_or(v).to_string());
            } else if let Some(v) = line.strip_prefix("event:") {
                self.name = Some(v.trim().to_string());
            }
            // (comments ": keepalive", id: and retry: are not needed)
        }
        if self.buf.len() + self.data.iter().map(String::len).sum::<usize>() > EVENT_MAX {
            self.buf.clear();
            self.data.clear();
            self.name = None;
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plaintext_goes_only_to_this_computer() {
        let e = |url: &str| Endpoint { url: url.into(), ..Default::default() }.check();
        assert!(e("http://127.0.0.1:8642").is_ok());
        assert!(e("http://localhost:8642/v1").is_ok());
        assert!(e("http://[::1]:8642").is_ok());
        assert!(e("https://agent.example.org").is_ok());
        assert!(e("http://192.168.1.5:8642").is_err());
        assert!(e("http://agent.example.org").is_err());
        assert!(e("http://user:pw@127.0.0.1").is_err());
        assert!(e("file:///etc/passwd").is_err());
        assert!(e("http://127.0.0.1.example.org").is_err());
        let base = |url: &str| Endpoint { url: url.into(), ..Default::default() }.base();
        assert_eq!(base("http://localhost:8642/v1/"), "http://127.0.0.1:8642");
        assert_eq!(base("http://127.0.0.1:8642"), "http://127.0.0.1:8642");
    }

    #[test]
    fn the_endpoint_is_kept_private() {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!("trommi-chat-{}", crate::util::random_hex(6)));
        let e = Endpoint { url: "http://127.0.0.1:8642".into(), key: "k".into(), model: "hermes-agent".into() };
        let file = e.save(&dir, "hermes").unwrap();
        assert_eq!(std::fs::metadata(&file).unwrap().permissions().mode() & 0o777, 0o600);
        assert_eq!(std::fs::metadata(file.parent().unwrap()).unwrap().permissions().mode() & 0o777, 0o700);
        assert_eq!(Endpoint::load(&dir, "hermes"), Some(e.clone()));
        // a remote plaintext address is never loaded
        Endpoint { url: "http://10.0.0.2:8642".into(), ..e }.save(&dir, "hermes").unwrap();
        assert_eq!(Endpoint::load(&dir, "hermes"), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_stream_reads_as_events() {
        let mut p = Sse::default();
        let wire = "data: {\"choices\":[{\"delta\":{\"role\":\"assistant\"}}]}\n\n: keepalive\n\nevent: hermes.tool.progress\ndata: {\"tool\":\"terminal\",\"emoji\":\"💻\",\"label\":\"ls\",\"toolCallId\":\"c1\",\"status\":\"running\"}\n\ndata: {\"choices\":[{\"delta\":{\"content\":\"Hal\"}}]}\n\ndata: {\"choices\":[{\"delta\":{\"content\":\"lo\"}}]}\r\n\r\nevent: approval.request\ndata: {\"event\":\"approval.request\",\"run_id\":\"chatcmpl-1\",\"command\":\"rm x\",\"description\":\"delete\",\"choices\":[\"once\",\"deny\"]}\n\ndata: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}\n\ndata: [DONE]\n\n";
        let mut got = vec![];
        // (in pieces that cut lines and characters)
        for piece in wire.as_bytes().chunks(7) {
            for (n, d) in p.push(piece) {
                got.extend(events(n.as_deref(), &d));
            }
        }
        assert_eq!(
            got,
            vec![
                Event::Tool { id: "c1".into(), tool: "terminal".into(), label: "💻 ls".into(), done: false },
                Event::Text("Hal".into()),
                Event::Text("lo".into()),
                Event::Approval(Approval { run_id: "chatcmpl-1".into(), request_id: None, command: "rm x".into(), description: "delete".into() }),
                Event::Done { finish: "stop".into(), error: None },
            ]
        );
    }

    /// A fake OpenAI-compatible server on this computer: answers each request with `answer` and keeps what came.
    async fn fake(answer: &'static str) -> (String, std::sync::Arc<std::sync::Mutex<Vec<String>>>) {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", l.local_addr().unwrap());
        let got = std::sync::Arc::new(std::sync::Mutex::new(vec![]));
        let kept = got.clone();
        tokio::spawn(async move {
            while let Ok((mut c, _)) = l.accept().await {
                let kept = kept.clone();
                tokio::spawn(async move {
                    let mut buf = vec![0u8; 65536];
                    let mut n = 0;
                    // (head and body: until the body's length is in)
                    loop {
                        let r = c.read(&mut buf[n..]).await.unwrap_or(0);
                        n += r;
                        let text = String::from_utf8_lossy(&buf[..n]).to_string();
                        if let Some(i) = text.find("\r\n\r\n") {
                            let len = text.lines().find_map(|l| l.to_ascii_lowercase().strip_prefix("content-length:").map(|v| v.trim().parse::<usize>().unwrap_or(0))).unwrap_or(0);
                            if n >= i + 4 + len {
                                kept.lock().unwrap().push(text);
                                break;
                            }
                        }
                        if r == 0 {
                            return;
                        }
                    }
                    let _ = c.write_all(answer.as_bytes()).await;
                    let _ = c.shutdown().await;
                });
            }
        });
        (url, got)
    }

    #[tokio::test]
    async fn a_turn_streams_from_the_agents_server() {
        let (url, got) = fake("HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nX-Hermes-Session-Id: trommi-s1\r\n\r\ndata: {\"choices\":[{\"delta\":{\"content\":\"Guten \"}}]}\n\nevent: hermes.tool.progress\ndata: {\"tool\":\"web_search\",\"label\":\"weather\",\"toolCallId\":\"t1\",\"status\":\"running\"}\n\ndata: {\"choices\":[{\"delta\":{\"content\":\"Tag\"}}]}\n\ndata: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}\n\ndata: [DONE]\n\n").await;
        let e = Endpoint { url, key: "secret".into(), model: "hermes-agent".into() };
        let mut seen = vec![];
        e.turn("trommi-s1", "Wie ist das Wetter?", |ev| seen.push(ev)).await;
        assert_eq!(seen.first(), Some(&Event::Started));
        assert_eq!(seen.last(), Some(&Event::Done { finish: "stop".into(), error: None }));
        let words: String = seen.iter().filter_map(|e| if let Event::Text(t) = e { Some(t.as_str()) } else { None }).collect();
        assert_eq!(words, "Guten Tag");
        let req = got.lock().unwrap()[0].clone();
        assert!(req.starts_with("POST /v1/chat/completions "), "{req}");
        assert!(req.contains("authorization: Bearer secret") || req.contains("Authorization: Bearer secret"));
        assert!(req.to_ascii_lowercase().contains("x-hermes-session-id: trommi-s1"));
        let body: Value = serde_json::from_str(&req[req.find("\r\n\r\n").unwrap() + 4..]).unwrap();
        assert_eq!(body, json!({ "model": "hermes-agent", "stream": true, "messages": [{ "role": "user", "content": "Wie ist das Wetter?" }] }));
    }

    #[tokio::test]
    async fn a_refused_turn_says_why_and_is_not_started() {
        let (url, _) = fake("HTTP/1.1 401 Unauthorized\r\nContent-Type: application/json\r\nContent-Length: 44\r\n\r\n{\"error\":{\"message\":\"Invalid API key here\"}}").await;
        let e = Endpoint { url, key: "wrong".into(), model: "m".into() };
        let mut seen = vec![];
        e.turn("c", "hi", |ev| seen.push(ev)).await;
        assert_eq!(seen.len(), 1);
        let Event::Done { finish, error: Some(why) } = &seen[0] else { panic!("{seen:?}") };
        assert_eq!(finish, "error");
        assert!(why.contains("401") && why.contains("Invalid API key"), "{why}");
        // nobody listens: not started either
        let e = Endpoint { url: "http://127.0.0.1:9".into(), key: "k".into(), model: "m".into() };
        let mut seen = vec![];
        e.turn("c", "hi", |ev| seen.push(ev)).await;
        assert!(matches!(&seen[..], [Event::Done { .. }]), "{seen:?}");
    }

    #[tokio::test]
    async fn an_approval_goes_back_to_its_run() {
        let (url, got) = fake("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 2\r\n\r\n{}").await;
        let e = Endpoint { url, key: "k".into(), model: "m".into() };
        let a = Approval { run_id: "chatcmpl-ab/../x".into(), request_id: Some("r1".into()), command: "rm x".into(), description: String::new() };
        e.approve(&a, false).await.unwrap();
        let req = got.lock().unwrap()[0].clone();
        assert!(req.starts_with("POST /v1/runs/chatcmpl-abx/approval "), "{req}");
        let body: Value = serde_json::from_str(&req[req.find("\r\n\r\n").unwrap() + 4..]).unwrap();
        assert_eq!(body, json!({ "choice": "deny", "request_id": "r1" }));
    }
}
