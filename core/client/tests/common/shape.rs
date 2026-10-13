//! The stand-in hub's answers held against what the real hub answered (`tests/hub/*.json`, the codec's recorded
//! vectors): every answer the stand-in gives on a route that was recorded must have the recorded shape. At each
//! place of the JSON (`items[].kind`), the stand-in may only use members the real hub used there, must have every
//! member the real hub always had there, and a value must be of a type the real hub put there.
//!
//! This checks shape, not meaning: the stand-in's rules stay hand-written (see `hub.rs`).

use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::OnceLock;

#[derive(Default, Debug)]
struct Place {
    /// The types seen here.
    types: BTreeSet<&'static str>,
    /// Members of objects here: in how many of the objects each was.
    members: BTreeMap<String, usize>,
    objects: usize,
}

type Shape = BTreeMap<String, Place>;

fn type_of(v: &Value) -> &'static str {
    match v {
        Value::Null => "null",
        Value::Bool(_) => "bool",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

/// Objects the hub keys by an id (the KeyPackages of a claim, by device).
const MAPS: &[&str] = &[".key_packages"];

fn walk(v: &Value, path: &str, shape: &mut Shape) {
    let place = shape.entry(path.to_string()).or_default();
    place.types.insert(type_of(v));
    match v {
        Value::Object(o) => {
            place.objects += 1;
            for name in o.keys() {
                *place.members.entry(name.clone()).or_default() += 1;
            }
            for (name, v) in o {
                walk(v, &format!("{path}.{name}"), shape);
            }
            if MAPS.contains(&path) {
                // (its members are keys, not names: every one counts as the same place)
                let place = shape.entry(path.to_string()).or_default();
                place.members.clear();
                for (name, v) in o {
                    shape.remove(&format!("{path}.{name}"));
                    walk(v, &format!("{path}{{}}"), shape);
                }
            }
        }
        Value::Array(list) => {
            for v in list {
                walk(v, &format!("{path}[]"), shape);
            }
        }
        _ => {}
    }
}

/// Members the real hub sends only in cases no recording holds (spec/hub-api.md): an envelope's void record, a
/// Commit's RecoveryAuth outside a group's log.
const SOMETIMES: &[&str] = &["void_code", "recovery_auth"];

/// Types the real hub puts at a place in cases no recording holds (hub/src/delivery.rs): a helper session's
/// `parent` in the list of the room's groups, its main session's id.
const ALSO: &[(&str, &str)] = &[(".items[].parent", "string")];

/// The recorded family a request belongs to: the prefix of its vectors' names.
fn family(method: &str, path: &str) -> Option<&'static [&'static str]> {
    let path = path.split('?').next().unwrap_or(path);
    let segs: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    Some(match (method, segs.as_slice()) {
        ("GET", ["v1", "rooms", _, "challenge"]) => &["challenge-"],
        ("POST", ["v1", "rooms", _, "tokens"]) => &["token-"],
        ("GET", ["v1", "changes"]) => &["changes-"],
        ("GET", ["v1", "desk"]) => &["desk", "desk-archived"],
        ("GET", ["v1", "cards", _]) => &["object-card"],
        ("GET", ["v1", "welcomes"]) => &["welcomes"],
        ("GET", ["v1", "groups", _, "log"]) => &["group-log"],
        ("GET", ["v1", "groups", _, "chains", _]) => &["chain"],
        ("GET", ["v1", "groups", _, "info"]) => &["group-info"],
        ("POST", ["v1", "groups", "read"]) => &["groups-read"],
        ("POST", ["v1", "commits"]) => &["joins"],
        ("PUT", ["v1", "key-packages"]) => &["key-packages-held", "outbox-key-packages"],
        ("POST", ["v1", "key-packages", "claim"]) => &["claim"],
        ("POST", ["v1", "groups"]) => &["outbox-group-founding"],
        ("GET", ["v1", "rooms", _, "groups"]) => &["room-groups"],
        ("POST", ["v1", "envelopes"]) => &["outbox-envelope"],
        ("POST", ["v1", "groups", _, "commits"]) => &["outbox-commit", "outbox-external-commit"],
        ("POST", ["v1", "groups", _, "messages"]) => &["outbox-message", "outbox-relay"],
        ("POST", ["v1", "requests"]) => &["wish"],
        ("GET", ["v1", "requests"]) => &["wishes"],
        ("POST", ["v1", "groups", _, "reject"]) => &["reject"],
        _ => return None,
    })
}

/// The recorded 2xx answers, by vector name.
fn recorded() -> &'static Vec<(String, Value)> {
    static VECTORS: OnceLock<Vec<(String, Value)>> = OnceLock::new();
    VECTORS.get_or_init(|| {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/hub");
        let mut out = Vec::new();
        for file in std::fs::read_dir(dir).expect("tests/hub") {
            let path = file.expect("a file").path();
            if path.extension().is_none_or(|x| x != "json") {
                continue;
            }
            let vector: Value = serde_json::from_slice(&std::fs::read(&path).expect("a vector")).expect("a vector is JSON");
            if vector["status"] != 200 {
                continue;
            }
            let Some(answer) = vector["answer"].as_str().and_then(|text| serde_json::from_str::<Value>(text).ok()) else { continue };
            out.push((path.file_stem().expect("a name").to_string_lossy().into_owned(), answer));
        }
        assert!(out.len() > 30, "the recorded vectors are there: {}", out.len());
        out
    })
}

/// Panics when a 200 answer of the stand-in on a recorded route has not the recorded shape.
pub fn check(method: &str, path: &str, answer: &Value) {
    let Some(prefixes) = family(method, path) else { return };
    let mut real = Shape::new();
    let mut any = false;
    for (name, recorded) in recorded() {
        // (a family is a name, or that name and what follows a dash: `changes-all-0`, `welcomes-none`)
        if prefixes.iter().any(|p| name == p.trim_end_matches('-') || name.strip_prefix(p.trim_end_matches('-')).is_some_and(|rest| rest.starts_with('-'))) {
            walk(recorded, "", &mut real);
            any = true;
        }
    }
    assert!(any, "no recorded vector for {method} {path}");
    let mut mine = Shape::new();
    walk(answer, "", &mut mine);
    for (place, here) in &mine {
        // (below a place the real hub left empty or null in every recording nothing is known)
        let Some(there) = real.get(place) else {
            let parent = place.rsplit_once(['.', '[']).map_or("", |(parent, _)| parent);
            let known = real.contains_key(parent) || real.contains_key(parent.trim_end_matches('['));
            let sometimes = place.rsplit_once('.').is_some_and(|(_, name)| SOMETIMES.contains(&name));
            assert!(!known || sometimes || place.ends_with("[]"), "{method} {path}: the stand-in answers `{place}`, the real hub never did");
            continue;
        };
        for kind in &here.types {
            assert!(there.types.contains(kind) || ALSO.contains(&(place.as_str(), *kind)), "{method} {path}: `{place}` is {kind} at the stand-in, at the real hub {:?}", there.types);
        }
        for name in here.members.keys() {
            assert!(there.members.contains_key(name) || SOMETIMES.contains(&name.as_str()), "{method} {path}: the stand-in's `{place}.{name}` is no member at the real hub");
        }
        if here.objects > 0 {
            for (name, count) in &there.members {
                let always = *count == there.objects;
                assert!(!always || here.members.get(name) == Some(&here.objects), "{method} {path}: the real hub always has `{place}.{name}`, the stand-in not");
            }
        }
    }
}
