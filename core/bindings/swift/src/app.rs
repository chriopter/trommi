//! The app core (core/client/src/app): the whole app's logic as one object per room, `AppCore`. One call in, one
//! call out per event: the event as JSON text with its bytes beside it, the effects the same way
//! (core/client/README.md, "The app core").

use crate::guard::Guarded;
use crate::CoreError;
use trommi_client_core::app::edge::{self, Packet};
use trommi_client_core::app::App;

record! {
    /// JSON text and the bytes it names: a `Bytes` field in the JSON is the index of its bytes in `blobs`.
    pub struct AppPacket {
        pub json: String,
        pub blobs: Vec<Vec<u8>>,
    }
}

/// The app core of one room on this device. Calls come one at a time (the guard waits or refuses re-entry); a
/// panic inside closes it for good, and the shell starts a new one, which reads the stored state again.
#[cfg_attr(feature = "uniffi", derive(uniffi::Object))]
pub struct AppCore(Guarded<App>);

#[cfg_attr(feature = "uniffi", uniffi::export)]
impl AppCore {
    /// A new app core; it does nothing until the event `start`.
    #[cfg_attr(feature = "uniffi", uniffi::constructor)]
    #[allow(clippy::new_without_default)]
    pub fn new() -> Self {
        Self(Guarded::new(App::new()))
    }

    /// One event in (`now_ms`: the shell's clock), every effect out, in the order the shell runs them.
    /// `bad-format` when the event is not the contract's JSON.
    pub fn handle(&self, now_ms: u64, event: AppPacket) -> Result<AppPacket, CoreError> {
        self.0.run(|app| {
            let packet = Packet {
                json: event.json,
                blobs: event.blobs,
            };
            let out =
                edge::handle(app, now_ms, packet).map_err(|why| CoreError::bad_format(&why))?;
            Ok(AppPacket {
                json: out.json,
                blobs: out.blobs,
            })
        })
    }

    /// The view model as rendered so far, as JSON: what a shell's folded view must equal (conformance), and what a
    /// view that opens late starts from.
    pub fn view_json(&self) -> Result<String, CoreError> {
        self.0.run(|app| {
            serde_json::to_string(app.view()).map_err(|_| CoreError::internal("the view model"))
        })
    }

    /// Gives the app core up; every later call is refused with `internal`.
    pub fn close(&self) {
        self.0.close();
    }
}
