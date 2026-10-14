//! The files a render showed, with their references (the key included): what the App lets the person open and
//! share (`crate::files`). Collected while `views::render` runs; the references never enter the view model.

use serde_json::{Map, Value};
use std::cell::RefCell;
use std::collections::BTreeMap;

thread_local! {
    static SEEN: RefCell<Option<BTreeMap<String, Map<String, Value>>>> = const { RefCell::new(None) };
}

/// Starts collecting (a render begins).
pub fn begin() {
    SEEN.with(|s| *s.borrow_mut() = Some(BTreeMap::new()));
}

/// What was collected since [`begin`]; collecting stops.
pub fn take() -> BTreeMap<String, Map<String, Value>> {
    SEEN.with(|s| s.borrow_mut().take()).unwrap_or_default()
}

/// A model attachment the views show.
pub fn saw(attachment: &Value) {
    let Some(a) = attachment.as_object() else { return };
    let Some(id) = a.get("attachment_id").and_then(Value::as_str) else { return };
    if a.get("file_key").is_none() {
        return;
    }
    SEEN.with(|s| {
        if let Some(seen) = s.borrow_mut().as_mut() {
            seen.entry(id.to_string()).or_insert_with(|| a.clone());
        }
    });
}
