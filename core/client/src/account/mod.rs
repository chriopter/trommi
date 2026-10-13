//! The account's flows: found a room, open the stored one, sign in, join by invite, recover, pair a device, the
//! devices and invites. W3 implements them (sources: web `room.ts`, `account.ts`; iOS `RoomAccount.swift`,
//! `RoomDevices.swift`, `LiveRecovery.swift`; Linux `engine/signin.rs`). Every step of a flow is an effect and an
//! event like any other; a flow's state lives in the App, its progress in the view model.

use serde::{Deserialize, Serialize};

/// Text the person typed that must never reach a log: its `Debug` is redacted.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Secret(pub String);

impl std::fmt::Debug for Secret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Secret(<redacted>)")
    }
}

/// `Debug` names the step only: e-mail addresses, links with their secret and passwords stay out of every log.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "step", rename_all = "snake_case")]
pub enum AccountAction {
    /// A new account and room.
    Found {
        email: String,
        password: Secret,
    },
    SignIn {
        email: String,
        password: Secret,
    },
    /// Sign in with the Emergency Kit words instead of the password.
    SignInWithKit {
        email: String,
        words: Secret,
    },
    /// Join by an invite link (this device becomes a member once the emoji match).
    Join {
        link: Secret,
    },
    /// The six emoji match (or not) on both screens.
    ConfirmEmoji {
        invite_id: String,
        matches: bool,
    },
    /// Make an invite link for a new device of the person.
    InviteDevice,
    RemoveDevices {
        device_ids: Vec<String>,
    },
    /// Use a passkey instead of the password (where `Config.can.passkeys`).
    SignInWithPasskey {
        email: String,
    },
    AddPasskey,
    /// A self-join ask: allow or deny.
    AnswerAsk {
        nonce: String,
        allow: bool,
    },
    ReplaceRecoveryCode,
    SignOut,
    DeleteAccount {
        password: Secret,
    },
}

impl std::fmt::Debug for AccountAction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let step = serde_json::to_value(self)
            .ok()
            .and_then(|v| v.get("step").and_then(|s| s.as_str()).map(str::to_string))
            .unwrap_or_default();
        write!(f, "AccountAction({step})")
    }
}
