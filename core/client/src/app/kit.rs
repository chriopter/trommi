//! The Emergency Kit around the account's screens: whether it still waits to be saved (the room register `kit`,
//! `{pending: true}`, written once the account is founded and cleared by `KitKept`; every human device of the person
//! sees it, web `auth.mjs` kitGate before), and what its sheet names (the account's e-mail and id, read once with
//! `ReadAccount`, and the address its QR code holds). Self-join asks and the invite in view are filled in beside them
//! (`AccountView` is made whole by many steps; these fields are the App's to add on every render).

use super::{App, Effect, Phase};
use crate::account::AccountAction;
use crate::actions::Action;
use serde_json::{json, Value};

#[derive(Default)]
pub(super) struct Kit {
    /// The account was founded on this device: the register is written once the room is live.
    pub(super) mark: bool,
    /// The account was read once for the kit's sheet.
    read: bool,
}

/// The address an Emergency Kit's QR code holds: the app's recovery screen with the hub and the account's id filled
/// in, `https://<app>/#k1.<hub address, base64url of its UTF-8>.<account id, 32 hex>`. None for an id that is not one.
pub fn kit_address(app: &str, hub: &str, account: &str) -> Option<String> {
    let digits: String = account.chars().filter(|c| *c != '-').collect::<String>().to_ascii_lowercase();
    if digits.len() != 32 || !digits.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    Some(format!("{app}/#k1.{}.{digits}", trommi_core::ids::base64url_encode(hub.as_bytes())))
}

impl App {
    /// The register says the kit waits, or this device founded the account and has not written it yet.
    fn kit_due(&self) -> bool {
        let Some(engine) = self.engine.as_ref() else { return false };
        let me = engine.device().id();
        if !engine.roles().0.contains(&me) {
            return false;
        }
        self.kit.mark || self.board.model.human.get("kit").is_some_and(|r| r.value.get("pending") == Some(&Value::Bool(true)))
    }

    /// After the engine's reports: the register written once live, and the account read once for the kit's sheet.
    pub(super) fn kit_tend(&mut self, now_ms: u64, out: &mut Vec<Effect>) {
        if self.engine.is_none() || self.demo {
            return;
        }
        if self.kit.mark && self.next_view.phase == Phase::Live {
            self.kit.mark = false;
            self.run_action(now_ms, Action::RoomRegister { key: "kit".into(), value: json!({ "pending": true }) }, out);
        }
        let wants = self.next_view.account.step == "keep_kit" || self.kit_due();
        if wants && !self.kit.read && self.next_view.settings.login.is_none() && !self.changes_running() {
            self.kit.read = true;
            self.change_action(now_ms, &AccountAction::ReadAccount, out);
        }
    }

    /// `KitKept`: the words leave the view, and the register no longer says the kit waits.
    pub(super) fn kit_kept(&mut self, now_ms: u64, out: &mut Vec<Effect>) {
        if self.next_view.account.step == "keep_kit" {
            self.next_view.account = crate::views::AccountView { step: "none".into(), ..Default::default() };
        }
        self.kit.mark = false;
        let written = self.board.model.human.get("kit").is_some_and(|r| r.value.get("pending") == Some(&Value::Bool(true)));
        if written && self.engine.is_some() && !self.demo {
            self.run_action(now_ms, Action::RoomRegister { key: "kit".into(), value: Value::Null }, out);
        }
    }

    /// What the App adds to the account's view on every render: the kit's flag and sheet, the asks, the invite.
    pub(super) fn account_extras(&mut self) {
        let due = self.kit_due();
        let asks = self.asks_view();
        let invite = self.invite_view();
        let login = self.next_view.settings.login.clone();
        let hub = self.engine.as_ref().map(|e| e.hub().to_string());
        let a = &mut self.next_view.account;
        a.kit_due = due;
        a.asks = asks;
        a.invite = invite;
        a.kit_email = login.as_ref().and_then(|l| l.email.clone());
        a.kit_account = login.as_ref().map(|l| l.account.clone()).filter(|x| !x.is_empty());
        a.kit_address = match (&a.kit_account, &hub) {
            (Some(account), Some(hub)) => kit_address(super::invites::INVITE_APP, hub, account),
            _ => None,
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_kit_address_names_the_hub_and_the_id() {
        let a = kit_address("https://app.trommi.com", "https://hub.trommi.com", "0123ABCD-0123-4567-89ab-0123456789ab").unwrap();
        assert_eq!(a, "https://app.trommi.com/#k1.aHR0cHM6Ly9odWIudHJvbW1pLmNvbQ.0123abcd0123456789ab0123456789ab");
        assert_eq!(kit_address("https://app.trommi.com", "https://hub.trommi.com", "not an id"), None);
    }
}
