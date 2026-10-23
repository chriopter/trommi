//! Conformance scripts: one list of events and expectations, run against the core here and against every shell's
//! binding there, so each shell proves it shows the same thing.
//!
//! A script is JSON (core/client/tests/conformance/*.json):
//!
//! ```json
//! { "name": "an empty device shows the sign-in",
//!   "steps": [
//!     { "at": 1000, "event": { "type": "start", "config": { "platform": "test", "version": "0", "seed": 1 } } },
//!     { "expect_effects": [ { "type": "store", "id": 1, "op": { "op": "device_load" } } ] },
//!     { "at": 1001, "event": { "type": "store", "id": 1,
//!                              "result": { "ok": { "answer": "device", "revision": 0, "entries": [] } } } },
//!     { "expect_view": { "phase": "signed_out" } } ] }
//! ```
//!
//! - `event`: sent at `at` (the clock the shell passes to `handle`).
//! - `expect_effects`: the effects of the last event, without the `Render`s, each matched as below, same count.
//! - `expect_view`: the shell's folded view model (every `Render` so far applied with the fold rule of
//!   `ViewModel::apply`), serialised as JSON, matched as below.
//!
//! **Matching** ([`matches`]): an expected object needs each of its keys, matched again; an array needs the same
//! length and each element matched; anything else must be equal. Keys the script does not name are free, so a
//! script stays valid while W4 adds fields.
//!
//! A shell's harness is the same thirty lines in its language: send each event through its binding, fold each
//! `Render` into its own view store, compare. [`run`] is the reference.

use super::{App, Effect, Event};
use crate::views::ViewModel;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Script {
    pub name: String,
    pub steps: Vec<Step>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Step {
    Event { at: u64, event: Event },
    ExpectEffects { expect_effects: Vec<Value> },
    ExpectView { expect_view: Value },
}

/// Something that takes events and shows a view: the core with its own fold here, a binding in a shell's test.
pub trait Shell {
    fn send(&mut self, now_ms: u64, event: Event) -> Vec<Effect>;
    /// The view model as this shell folded it, as JSON.
    fn view(&self) -> Value;
}

/// The reference shell: the App, and a view model folded only from the `Render`s it handed out.
#[derive(Default)]
pub struct CoreShell {
    pub app: App,
    pub folded: ViewModel,
}

impl Shell for CoreShell {
    fn send(&mut self, now_ms: u64, event: Event) -> Vec<Effect> {
        let effects = self.app.handle(now_ms, event);
        for e in &effects {
            if let Effect::Render { delta, .. } = e {
                self.folded.apply(delta).expect("the core renders in order");
            }
        }
        effects
    }
    fn view(&self) -> Value {
        serde_json::to_value(&self.folded).expect("the view is plain data")
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Mismatch {
    pub script: String,
    pub step: usize,
    pub path: String,
    pub expected: Value,
    pub got: Value,
}

impl std::fmt::Display for Mismatch {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} step {}: at {} expected {} got {}",
            self.script, self.step, self.path, self.expected, self.got
        )
    }
}

/// Where `got` fails to match `expected`, as a path ("$.desk.stack[1]"), or `None` when it matches.
pub fn matches(expected: &Value, got: &Value) -> Option<String> {
    fn walk(e: &Value, g: &Value, path: &mut String) -> bool {
        match (e, g) {
            (Value::Object(e), Value::Object(g)) => e.iter().all(|(k, ev)| {
                let len = path.len();
                path.push('.');
                path.push_str(k);
                let ok = g.get(k).is_some_and(|gv| walk(ev, gv, path));
                if ok {
                    path.truncate(len);
                }
                ok
            }),
            (Value::Array(e), Value::Array(g)) => {
                e.len() == g.len()
                    && e.iter().zip(g).enumerate().all(|(i, (ev, gv))| {
                        let len = path.len();
                        path.push_str(&format!("[{i}]"));
                        let ok = walk(ev, gv, path);
                        if ok {
                            path.truncate(len);
                        }
                        ok
                    })
            }
            _ => e == g,
        }
    }
    let mut path = String::from("$");
    (!walk(expected, got, &mut path)).then_some(path)
}

/// Runs a script against a shell. The first mismatch ends it.
pub fn run(script: &Script, shell: &mut dyn Shell) -> Result<(), Mismatch> {
    let mut last: Vec<Value> = Vec::new();
    for (step, s) in script.steps.iter().enumerate() {
        let fail = |path: String, expected: &Value, got: Value| Mismatch {
            script: script.name.clone(),
            step,
            path,
            expected: expected.clone(),
            got,
        };
        match s {
            Step::Event { at, event } => {
                last = shell
                    .send(*at, event.clone())
                    .iter()
                    .filter(|e| !matches!(e, Effect::Render { .. }))
                    .map(|e| serde_json::to_value(e).expect("plain data"))
                    .collect();
            }
            Step::ExpectEffects { expect_effects } => {
                let expected = Value::Array(expect_effects.clone());
                let got = Value::Array(last.clone());
                if let Some(path) = matches(&expected, &got) {
                    return Err(fail(path, &expected, got));
                }
            }
            Step::ExpectView { expect_view } => {
                let got = shell.view();
                if let Some(path) = matches(expect_view, &got) {
                    return Err(fail(path, expect_view, got));
                }
            }
        }
    }
    Ok(())
}
