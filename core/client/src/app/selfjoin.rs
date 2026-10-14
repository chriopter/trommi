//! Self-join on the person's side, done (spec/v1.md 12.4; the rules are `crate::selfjoin`; web `client.ts`
//! `answerAsks`, `answerVouch`, `vouchConfirm`, `retireDevice`): an agent's ask past the bounds is refused at once
//! (`join_link/<nonce>` = `{ refused }` in the asking session's group); `AnswerAsk` allows one (an agent invite
//! vouched for by that session, its link written for the agent once the hub holds it) or denies it; the Request of a
//! vouched link is let in when the ask names its hash and the voucher still stands, and burned otherwise; the device
//! of an archived session that joined by itself goes. Only while live, and each ask answered once by this device.

use super::invites::Vouch;
use super::{App, Effect, Phase};
use crate::account::AccountAction;
use serde_json::{json, Value};
use std::collections::BTreeSet;

#[derive(Default)]
pub(super) struct SelfJoin {
    /// The asks this device answered (allowed, denied or refused at once).
    answered: BTreeSet<String>,
    /// Devices being removed (an archived self-joined session's).
    retiring: BTreeSet<String>,
    /// Vouched invites whose verdict was acted on.
    decided: BTreeSet<String>,
}

impl App {
    /// `AnswerAsk`: true when `action` was one.
    pub(super) fn selfjoin_action(&mut self, now_ms: u64, action: &AccountAction, out: &mut Vec<Effect>) -> bool {
        let AccountAction::AnswerAsk { nonce, allow } = action else { return false };
        let (pending, _) = self.vouched_open();
        let asks = crate::selfjoin::asks(&self.board.model, &self.selfjoin.answered, &pending);
        let Some(ask) = asks.into_iter().find(|a| a.nonce == *nonce) else { return true };
        self.selfjoin.answered.insert(nonce.clone());
        let refused = match (&ask.refused, *allow) {
            (Some(why), _) => Some(why.clone()),
            (None, false) => Some("denied".to_string()),
            (None, true) => None,
        };
        if let Some(why) = refused {
            self.answer_ask(now_ms, &ask.session_id, nonce, json!({ "refused": why }), out);
            return true;
        }
        let s = self.board.model.sessions.get(&ask.session_id);
        let settings = s.and_then(|s| s.settings.clone()).unwrap_or(Value::Null);
        let text = |v: &Value, k: &str| v.get(k).and_then(Value::as_str).unwrap_or("").to_string();
        let vouch = Vouch {
            nonce: nonce.clone(),
            session_id: ask.session_id.clone(),
            account: text(&ask.ask, "account"),
            account_label: text(&ask.ask, "account_label"),
            by_name: Some(text(&settings, "name")).filter(|n| !n.is_empty()).unwrap_or_else(|| text(&ask.ask, "vouched_by")),
        };
        let label: String = ask.name.chars().take(120).collect();
        let desk = settings.get("desk").and_then(Value::as_str).map(String::from);
        if !self.invite_open(now_ms, trommi_core::invite::Role::Agent, label, desk, Some(vouch), out) {
            self.answer_ask(now_ms, &ask.session_id, nonce, json!({ "refused": "busy" }), out);
        }
        true
    }

    fn answer_ask(&mut self, now_ms: u64, session_id: &str, nonce: &str, value: Value, out: &mut Vec<Effect>) {
        self.doing(now_ms, out, |doing| {
            doing.session_register(session_id, &format!("join_link/{nonce}"), &value);
        });
    }

    /// After every look at the board while live: asks past the bounds refused, vouched Requests decided, the devices
    /// of archived self-joined sessions removed.
    pub(super) fn selfjoin_tend(&mut self, now_ms: u64, out: &mut Vec<Effect>) {
        if self.next_view.phase != Phase::Live || self.engine.is_none() {
            return;
        }
        let (pending, waiting) = self.vouched_open();
        let refusals: Vec<(String, String, String)> = crate::selfjoin::asks(&self.board.model, &self.selfjoin.answered, &pending)
            .into_iter()
            .filter_map(|a| a.refused.map(|why| (a.session_id, a.nonce, why)))
            .collect();
        for (session, nonce, why) in refusals {
            self.selfjoin.answered.insert(nonce.clone());
            self.answer_ask(now_ms, &session, &nonce, json!({ "refused": why }), out);
        }
        for (invite_id, vouch, hash) in waiting {
            if self.selfjoin.decided.contains(&invite_id) {
                continue;
            }
            let Some(verdict) = crate::selfjoin::verdict(&self.board.model, &vouch.session_id, &vouch.nonce, &hash) else { continue };
            self.selfjoin.decided.insert(invite_id.clone());
            self.invite_confirm(now_ms, &invite_id, verdict, out);
        }
        let agents = self.engine.as_ref().map(|e| e.roles().1).unwrap_or_default();
        for device in crate::selfjoin::to_retire(&self.board.model) {
            let present = agents.iter().any(|a| a.as_bytes().iter().map(|b| format!("{b:02x}")).collect::<String>() == device);
            if present && self.selfjoin.retiring.insert(device.clone()) {
                self.account_step(now_ms, &AccountAction::RemoveDevices { device_ids: vec![device] }, out);
            }
        }
    }
}
