//! The account's flows: found a room, open the stored one, sign in, join by invite, recover, pair a device, the
//! devices and invites. W3 implements them (sources: web `room.ts`, `account.ts`; iOS `RoomAccount.swift`,
//! `RoomDevices.swift`, `LiveRecovery.swift`; Linux `engine/signin.rs`). Every step of a flow is an effect and an
//! event like any other; a flow's state lives in the App, its progress in the view model.

use serde::{Deserialize, Serialize};

pub mod found;
pub mod signin;

/// Text the person typed that must never reach a log: its `Debug` is redacted.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Secret(pub String);

impl std::fmt::Debug for Secret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Secret(<redacted>)")
    }
}

/// The way in a change of the account is proved with, given again.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "by", rename_all = "snake_case")]
pub enum Unlock {
    Password { password: Secret },
    /// The Emergency Kit's twelve words.
    Words { words: Secret },
    /// One of the account's passkeys (`PlatformOp::PasskeyGet`).
    Passkey,
}

/// `Debug` names the step only: e-mail addresses, links with their secret and passwords stay out of every log.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "step", rename_all = "snake_case")]
pub enum AccountAction {
    /// A new account and room.
    Found {
        email: String,
        password: Secret,
        /// The hub to found the room on; none: `Config.hub`, else the default hub.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        hub: Option<String>,
    },
    /// The person has kept the Emergency Kit's words shown once after founding: they leave the view.
    KitKept,
    SignIn {
        email: String,
        password: Secret,
        /// The hub to sign in at, for a shell whose `Config.hub` is empty and that lets the person name one;
        /// none: `Config.hub`, else the default hub.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        hub: Option<String>,
    },
    /// Sign in with the Emergency Kit words instead of the password. `email`: the account's e-mail, or its id
    /// (as printed on the kit) for an account without one.
    SignInWithKit {
        email: String,
        words: Secret,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        hub: Option<String>,
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
    /// Make an invite link for an agent, which founds its main session; `label` names that session, `desk` places it.
    InviteAgent {
        #[serde(default)]
        label: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        desk: Option<String>,
    },
    RemoveDevices {
        device_ids: Vec<String>,
    },
    /// Use a passkey instead of the password (where `Config.can.passkeys`): the platform offers the person's
    /// passkeys for the hub (`PlatformOp::PasskeyGet`).
    SignInWithPasskey {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        hub: Option<String>,
    },
    /// Reads the account (`GET /v1/account`) into `view.settings.login`: its e-mail, password, kit and passkeys.
    ReadAccount,
    /// A new Emergency Kit for the same recovery code; its words are shown once (`step: "keep_kit"`).
    NewKit {
        unlock: Unlock,
    },
    /// Sets the password: a change with the password of now, or a new one with a passkey or the kit's words. With
    /// the words the recovery code is replaced as well (8.6), and a new kit is shown.
    SetPassword {
        unlock: Unlock,
        next: Secret,
    },
    /// Gives an account without e-mail one, once; the kit's same words are typed again (they are salted with the
    /// e-mail from now on).
    SetEmail {
        email: String,
        words: Secret,
    },
    /// Adds a passkey (`PlatformOp::PasskeyCreate`). With the kit's words the code is replaced as well, and a new
    /// kit is shown.
    AddPasskey {
        unlock: Unlock,
    },
    /// Removes a passkey (its id, base64url).
    RemovePasskey {
        credential_id: String,
        unlock: Unlock,
    },
    /// A self-join ask: allow or deny.
    AnswerAsk {
        nonce: String,
        allow: bool,
    },
    /// Replaces the room's recovery code (8.6), after a device not in the person's hands was removed: the way in
    /// given (the password or a passkey) is kept, every other is removed, and a new kit is shown.
    ReplaceRecoveryCode {
        unlock: Unlock,
    },
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
