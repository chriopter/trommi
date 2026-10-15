//! The drawings a session can wear as its symbol, from connector/drawings.json: the one list the agent's tool
//! descriptions, the check of a given icon and the default by session name all come from. The apps hold the same
//! names in the same order (app/web/public/ui.mjs, app/ios/.../Pen.swift); a test keeps the three equal.
use serde_json::Value;
use std::sync::OnceLock;

pub const DRAWINGS_JSON: &str = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/drawings.json"));

pub struct Drawing {
    pub name: String,
    pub meaning: String,
    pub topics: Vec<String>,
}

pub fn all() -> &'static [Drawing] {
    static D: OnceLock<Vec<Drawing>> = OnceLock::new();
    D.get_or_init(|| {
        let v: Value = serde_json::from_str(DRAWINGS_JSON).expect("drawings.json");
        let text = |x: &Value, k: &str| x[k].as_str().unwrap_or("").to_string();
        v.as_array()
            .expect("drawings.json is a list")
            .iter()
            .map(|x| Drawing {
                name: text(x, "name"),
                meaning: text(x, "meaning"),
                topics: x["topics"]
                    .as_array()
                    .map(|t| t.iter().filter_map(|s| s.as_str().map(String::from)).collect())
                    .unwrap_or_default(),
            })
            .collect()
    })
}

/// "burst (spark), spiral (loop), …": the names with a word each, for the tools' `icon` description.
pub fn listing() -> String {
    all()
        .iter()
        .map(|d| format!("{} ({})", d.name, d.meaning))
        .collect::<Vec<_>>()
        .join(", ")
}

/// The mark the board stores for a drawing name: "draw:rocket". Accepts "rocket", "Rocket" and "draw:rocket".
/// An unknown name is an error that lists the valid ones.
pub fn mark_of(given: &str) -> Result<String, String> {
    let g = given.trim().to_lowercase();
    let bare = g.strip_prefix("draw:").unwrap_or(&g).trim().to_string();
    if all().iter().any(|d| d.name == bare) {
        return Ok(format!("draw:{bare}"));
    }
    Err(format!(
        "no drawing named \"{given}\". Pass one of these names (no prefix needed): {}",
        listing()
    ))
}

/// The drawing that fits a session's name ("CI" -> branch, "Web" -> browser), or None when nothing fits; the
/// board then keeps its seeded scribble. Matches the whole name or any word of it, ignoring case.
pub fn default_for(session_name: &str) -> Option<String> {
    let lower = session_name.trim().to_lowercase();
    let words: Vec<&str> = lower
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .collect();
    for d in all() {
        if d.name == lower || d.topics.contains(&lower) {
            return Some(format!("draw:{}", d.name));
        }
    }
    for w in words {
        for d in all() {
            if d.name == w || d.topics.iter().any(|t| t == w) {
                return Some(format!("draw:{}", d.name));
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_unique_and_topics_do_not_collide() {
        let mut seen = std::collections::HashSet::new();
        for d in all() {
            assert!(seen.insert(d.name.clone()), "{} twice", d.name);
            assert!(!d.meaning.is_empty() && !d.meaning.contains(' '), "{}: one word", d.name);
        }
        let mut topics = std::collections::HashMap::new();
        for d in all() {
            for t in &d.topics {
                if let Some(o) = topics.insert(t.clone(), d.name.clone()) {
                    panic!("topic {t} for both {o} and {}", d.name);
                }
            }
        }
        assert_eq!(all().len(), 52);
    }

    #[test]
    fn a_given_name_gets_its_prefix_and_an_unknown_one_lists_the_valid() {
        assert_eq!(mark_of("rocket").unwrap(), "draw:rocket");
        assert_eq!(mark_of(" Draw:Rocket ").unwrap(), "draw:rocket");
        let e = mark_of("spaceship").unwrap_err();
        assert!(e.contains("spaceship") && e.contains("rocket (release)") && e.contains("burst (spark)"), "{e}");
    }

    #[test]
    fn the_default_follows_the_session_name() {
        for (n, d) in [("CI", "branch"), ("Web", "browser"), ("iOS", "phone"), ("Design", "brush"), ("Docs", "book"),
            ("Video", "mic"), ("Mac", "anchor"), ("Linux", "terminal"), ("Core", "lock"), ("Connector", "arrow"),
            ("Release", "rocket"), ("web-tests", "browser"), ("Trommi", "bell")] {
            assert_eq!(default_for(n).as_deref(), Some(format!("draw:{d}").as_str()), "{n}");
        }
        assert_eq!(default_for("Zebra"), None);
        assert_eq!(default_for(""), None);
    }

    /// The web and the iOS app hold the same drawings, in the same order, as drawings.json.
    #[test]
    fn web_and_ios_hold_the_same_list() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
        let want: Vec<String> = all().iter().map(|d| d.name.clone()).collect();

        let ui = std::fs::read_to_string(root.join("app/web/public/ui.mjs")).unwrap();
        let kinds = ui.split("const KINDS = [").nth(1).unwrap().split(']').next().unwrap();
        let mut web: Vec<String> = kinds.split(',').filter_map(|s| s.split('\'').nth(1).map(String::from)).collect();
        let named = ui.split("const NAMED = {").nth(1).unwrap().split("\nconst KINDS").next().unwrap();
        web.extend(named.lines().filter(|l| l.starts_with("  ") && !l.starts_with("   ") && !l.trim_start().starts_with("//"))
            .filter_map(|l| l.trim().split(':').next().map(String::from)));
        assert_eq!(web, want, "app/web/public/ui.mjs");

        let pen = std::fs::read_to_string(root.join("app/ios/TrommiApp/Sources/TrommiApp/Pen.swift")).unwrap();
        let kinds_swift = pen.split("static let kinds").nth(1).unwrap().split(']').next().unwrap().to_string();
        let drawings = pen.split("static let drawings = kinds + [").nth(1).unwrap().split(']').next().unwrap().to_string();
        let quoted = |s: &str| -> Vec<String> { s.split('"').skip(1).step_by(2).map(String::from).collect() };
        let mut ios = quoted(&kinds_swift);
        ios.extend(quoted(&drawings));
        assert_eq!(ios, want, "Pen.swift");
    }
}
