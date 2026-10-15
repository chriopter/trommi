//! Test-only HTTP peer for the human stand-in and deliberately malformed hub requests.
//! Authentication and stream parsing use the shared core; application code uses Driver and trommi-io.
use serde_json::{json, Value};
use std::{collections::VecDeque, sync::Arc, time::{Duration, Instant}};
use trommi_connector::{driver::{BoxFut, OpenHub}, error::{Fault, Result}, keeper::Keeper, vault::Vault};
use trommi_core::{hub_auth::{HubAddress, SignedHubAuth}, ids::RoomId};
use trommi_client_core::{app::io::Header, hub::{StreamEvent as CoreEvent, StreamReader}};

pub use trommi_connector::driver::b64;
pub fn unb64(value: &Value, field: &str) -> Result<Vec<u8>> {
    trommi_core::ids::base64url_decode(value[field].as_str().ok_or_else(|| Fault::new("bad-format", "missing bytes"))?).map_err(Into::into)
}
type Signer = Arc<dyn Fn([u8; 32]) -> BoxFut<'static, Result<SignedHubAuth>> + Send + Sync>;

pub struct Hub {
    address: HubAddress,
    http: reqwest::Client,
    room: Option<RoomId>,
    signer: Option<Signer>,
    token: tokio::sync::Mutex<Option<(Instant, String)>>,
}
impl Hub {
    pub fn new(address: &str, room: Option<RoomId>, signer: Option<Signer>) -> Result<Self> {
        Ok(Self { address: HubAddress::parse(address)?, http: reqwest::Client::new(), room, signer, token: Default::default() })
    }
    async fn token(&self) -> Result<Option<String>> {
        let (Some(room), Some(sign)) = (self.room, self.signer.as_ref()) else { return Ok(None) };
        let mut token = self.token.lock().await;
        if let Some((at, value)) = token.as_ref().filter(|(at, _)| at.elapsed() < Duration::from_secs(600)) {
            let _ = at;
            return Ok(Some(value.clone()));
        }
        let value = OpenHub::new(self.address.as_str())?.sign_in(&room, |challenge| sign(challenge)).await?;
        *token = Some((Instant::now(), value.clone()));
        Ok(Some(value))
    }
    async fn send(&self, method: reqwest::Method, path: &str, body: Option<&Value>, signed: bool) -> Result<Value> {
        let mut request = self.http.request(method, format!("{}{path}", self.address.as_str())).timeout(Duration::from_secs(30));
        if let Some(body) = body { request = request.header("content-type", "application/json").body(serde_json::to_vec(body)?); }
        let unsigned = request.try_clone().ok_or_else(|| Fault::plain("test request cannot be cloned"))?;
        if signed { if let Some(token) = self.token().await? { request = request.bearer_auth(token); } }
        let mut response = request.try_clone().ok_or_else(|| Fault::plain("test request cannot be cloned"))?.send().await.map_err(|_| Fault::new("offline", "test hub did not answer"))?;
        if signed && response.status() == reqwest::StatusCode::UNAUTHORIZED {
            *self.token.lock().await = None;
            request = unsigned;
            if let Some(token) = self.token().await? { request = request.bearer_auth(token); }
            response = request.send().await.map_err(|_| Fault::new("offline", "test hub did not answer"))?;
        }
        let status = response.status().as_u16();
        let retry = response.headers().get("retry-after").and_then(|v| v.to_str().ok()).and_then(|v| v.parse().ok());
        let bytes = response.bytes().await.map_err(|_| Fault::new("offline", "test hub response cut"))?;
        let value: Value = if bytes.is_empty() { json!({}) } else { serde_json::from_slice(&bytes)? };
        if !(200..300).contains(&status) {
            let mut fault = Fault::new(value["error"].as_str().unwrap_or("hub-error"), "test hub refused request").status(status);
            fault.retry_after = retry;
            fault.extra = value.as_object().cloned().unwrap_or_default();
            return Err(fault);
        }
        Ok(value)
    }
    pub async fn open_call(&self, method: reqwest::Method, path: &str, body: Option<&Value>) -> Result<Value> { self.send(method, path, body, false).await }
    pub async fn get(&self, path: &str) -> Result<Value> { self.send(reqwest::Method::GET, path, None, true).await }
    pub async fn post(&self, path: &str, body: &Value) -> Result<Value> { self.send(reqwest::Method::POST, path, Some(body), true).await }
    pub async fn put(&self, path: &str, body: &Value) -> Result<Value> { self.send(reqwest::Method::PUT, path, Some(body), true).await }
    pub async fn delete(&self, path: &str) -> Result<Value> { self.send(reqwest::Method::DELETE, path, None, true).await }
    pub async fn stream(&self, after: u64) -> Result<Stream> {
        let mut request = self.http.get(format!("{}/v1/stream?after={after}", self.address.as_str()));
        if let Some(token) = self.token().await? { request = request.bearer_auth(token); }
        let response = request.send().await.map_err(|_| Fault::new("offline", "test stream did not open"))?;
        let headers = response.headers().iter().filter_map(|(k,v)| Some(Header { name: k.to_string(), value: v.to_str().ok()?.to_string() })).collect::<Vec<_>>();
        let mut reader = StreamReader::new(after);
        reader.opened(response.status().as_u16(), &headers).map_err(|e| trommi_connector::driver::fault_of(&e))?;
        Ok(Stream { response, reader, events: VecDeque::new() })
    }
}
pub fn hub_for(vault: &Arc<Keeper<Vault>>, address: &str, room: RoomId) -> Result<Hub> {
    let parsed = HubAddress::parse(address)?;
    let vault = vault.clone();
    let sign: Signer = Arc::new(move |challenge| {
        let (vault, parsed) = (vault.clone(), parsed.clone());
        Box::pin(async move { Ok(vault.call(move |v: &mut Vault| v.device.hub_sign_in(&parsed, challenge)).await??) })
    });
    Hub::new(address, Some(room), Some(sign))
}
pub struct StreamEvent { pub name: String, pub data: Value }
pub struct Stream { response: reqwest::Response, reader: StreamReader, events: VecDeque<StreamEvent> }
impl Stream {
    pub async fn next(&mut self, silence: Duration) -> Result<Option<StreamEvent>> {
        loop {
            if let Some(event) = self.events.pop_front() { return Ok(Some(event)); }
            let Some(bytes) = tokio::time::timeout(silence, self.response.chunk()).await.map_err(|_| Fault::new("timeout", "test stream silent"))?.map_err(|_| Fault::new("offline", "test stream cut"))? else { return Ok(None) };
            for event in self.reader.chunk(&bytes).into_result().map_err(|e| trommi_connector::driver::fault_of(&e))? {
                if let CoreEvent::Presence(p) = event {
                    self.events.push_back(StreamEvent { name: "presence".into(), data: json!({ "device": p.device.to_base64url(), "online": p.online }) });
                }
            }
        }
    }
}
