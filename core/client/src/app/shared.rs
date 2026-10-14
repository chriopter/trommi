//! Opening a Share link (spec/v1.md 11.5) for someone outside the room (`Action::OpenShared`, the web's page
//! `/artifact/<share id>#…`): no device, no token. The link names the share and holds its secret, the file's key and
//! its hash; the hub serves the stored file for the secret alone (`GET /v1/shares/{id}`, the secret in a header), and
//! it is checked against the hash and decrypted here. Its bytes go to the shell as `FileReady` under [`SHARED_FILE`];
//! a link that is not whole, one the hub no longer serves, or a file that does not open is said by a toast.

use super::{log, App, Effect, LogLevel, Pending};
use crate::app::io::{HttpResponse, IoResult};
use crate::hub::{Answer, Codec, Expect, Failure, Route, Site};
use crate::views::Text;
use trommi_core::files::{FileRef, ShareLink};
use trommi_core::ids::FileId;

/// The file id the shared file's bytes come under.
pub const SHARED_FILE: &str = "shared";

impl App {
    /// `OpenShared`: the request for the shared file.
    pub(super) fn open_shared(&mut self, link: &str, out: &mut Vec<Effect>) {
        let Some(config) = self.config.as_ref() else { return };
        let client = format!("{}/{}", config.platform, config.version);
        let hub = crate::account::signin::hub_of(None, &config.hub);
        let parsed = ShareLink::parse(link.trim());
        let (Ok(hub), Ok(share)) = (hub, parsed) else {
            return self.shared_failed(Text::new("share.bad_link", "This link is incomplete: the part after # is missing or damaged."));
        };
        let site = Site { url: &hub, token: None, client: &client, lease: None };
        match Codec.request(&site, &Route::GetShared { share: &share.share_id, secret: share.secret.expose(), range: None }) {
            Ok((request, expect)) => {
                let id = self.io(Pending::Shared);
                self.shared_ask = Some((share, expect));
                out.push(Effect::Http { id, request });
            }
            Err(_) => self.shared_failed(Text::new("share.bad_link", "This link is incomplete: the part after # is missing or damaged.")),
        }
    }

    /// The hub's answer: the file decrypted for the shell, or why not.
    pub(super) fn shared_answered(&mut self, link: ShareLink, expect: Expect, result: IoResult<HttpResponse>, out: &mut Vec<Effect>) {
        let answer = match result {
            IoResult::Ok(response) => Codec.answer(&expect, &response),
            IoResult::Err(error) => Err(crate::hub::classify_io(&error).unwrap_or(Failure::Offline)),
        };
        match answer {
            Ok(Answer::File(file)) => {
                // (a Share link names no file id: the stored file's head does, and the hash checks it)
                let Some(file_id) = file.bytes.get(1..17).and_then(|b| <[u8; 16]>::try_from(b).ok()).map(FileId::new) else {
                    return self.shared_failed(Text::new("share.broken", "This file does not match the link."));
                };
                let reference = FileRef { file_id, file_key: link.file_key.duplicate(), sha256: link.sha256 };
                match trommi_core::files::decrypt_file(&reference, &file.bytes) {
                    Ok(plain) => out.push(Effect::FileReady { file_id: SHARED_FILE.into(), media_type: String::new(), bytes: plain.into() }),
                    Err(_) => self.shared_failed(Text::new("share.broken", "This file does not match the link.")),
                }
            }
            Err(Failure::Refused { status: 404, .. }) => self.shared_failed(Text::new("share.gone", "This link has expired or was withdrawn.")),
            other => {
                out.push(log(LogLevel::Info, &format!("share: {:?}", other.err())));
                self.shared_failed(Text::new("share.failed", "The file could not be opened. Try again."))
            }
        }
    }

    fn shared_failed(&mut self, text: Text) {
        self.sent.toast(&mut self.next_view.toasts, text, None);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::actions::Action;
    use crate::app::sim::Sim;
    use crate::app::{Config, Event, HttpResponse};
    use crate::views::Phase;
    use trommi_core::crypto::SystemEntropy;

    fn signed_out() -> Sim {
        let mut sim = Sim::new(1_000);
        sim.send(Event::Start { config: Config { platform: "test".into(), version: "0".into(), hub: "https://hub.example".into(), seed: 1, ..Default::default() } });
        assert_eq!(sim.view.phase, Phase::SignedOut);
        sim
    }

    /// The hub: the stored file for the link's secret, a 404 for anything else.
    fn serving(sim: &mut Sim, link: &ShareLink, stored: Vec<u8>) {
        let path = format!("/v1/shares/{}", trommi_core::ids::base64url_encode(link.share_id.as_bytes()));
        let secret = String::from_utf8(link.secret_base64url().expose().to_vec()).unwrap();
        sim.hub = Some(Box::new(move |_, request| {
            let ok = request.url.ends_with(&path) && request.headers.iter().any(|h| h.name == "x-share-secret" && h.value == secret);
            let (status, body) = if ok { (200, stored.clone()) } else { (404, br#"{"error":{"code":"not-found","message":"no such share"}}"#.to_vec()) };
            Ok(HttpResponse { status, headers: vec![], body: body.into() })
        }));
    }

    #[test]
    fn a_share_link_opens_without_a_device() {
        let plain = b"a page for someone outside the room".to_vec();
        let (stored, sealed) = trommi_core::files::encrypt_file(&plain, &mut SystemEntropy).unwrap();
        let link = ShareLink::create("https://app.trommi.com", &sealed.file, &mut SystemEntropy).unwrap();
        let text = String::from_utf8(link.to_text().expose().to_vec()).unwrap();
        let mut sim = signed_out();
        serving(&mut sim, &link, stored.clone());
        sim.send(Event::Action { action: Action::OpenShared { link: text.clone() } });
        let got = sim.log.iter().find_map(|e| match e {
            Effect::FileReady { file_id, bytes, .. } => Some((file_id.clone(), bytes.0.clone())),
            _ => None,
        });
        assert_eq!(got, Some((SHARED_FILE.to_string(), plain)));
        assert!(sim.view.toasts.is_empty(), "{:?}", sim.view.toasts);
    }

    #[test]
    fn a_damaged_ended_or_wrong_link_is_said() {
        let (stored, sealed) = trommi_core::files::encrypt_file(b"bytes", &mut SystemEntropy).unwrap();
        let link = ShareLink::create("https://app.trommi.com", &sealed.file, &mut SystemEntropy).unwrap();
        let text = String::from_utf8(link.to_text().expose().to_vec()).unwrap();
        let said = |sim: &Sim| sim.view.toasts.iter().map(|t| t.text.key.clone()).collect::<Vec<_>>();

        let mut sim = signed_out();
        serving(&mut sim, &link, stored.clone());
        sim.send(Event::Action { action: Action::OpenShared { link: text.split('#').next().unwrap().to_string() } });
        assert_eq!(said(&sim), ["share.bad_link"]);

        // another link's secret: the hub serves nothing
        let other = ShareLink::create("https://app.trommi.com", &sealed.file, &mut SystemEntropy).unwrap();
        let mut sim = signed_out();
        serving(&mut sim, &other, stored.clone());
        sim.send(Event::Action { action: Action::OpenShared { link: text.clone() } });
        assert_eq!(said(&sim), ["share.gone"]);

        // the hub hands other bytes: they do not match the link's hash
        let (wrong, _) = trommi_core::files::encrypt_file(b"other bytes", &mut SystemEntropy).unwrap();
        let mut sim = signed_out();
        serving(&mut sim, &link, wrong);
        sim.send(Event::Action { action: Action::OpenShared { link: text } });
        assert_eq!(said(&sim), ["share.broken"]);
        assert!(!sim.log.iter().any(|e| matches!(e, Effect::FileReady { .. })));
    }
}
