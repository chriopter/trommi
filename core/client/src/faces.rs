//! The agents' faces: the 24 hand-drawn faces a session wears as its mark, their words for the agents, and which
//! face a session gets. Every app draws a face by its name ("draw:cloud"); the pens hold the strokes
//! (app/web/public/ui.mjs `FACES`, the iOS app's pen.json and the Linux pen are made from it) in this order, and the
//! connector offers these names to the agents. A test in the connector keeps the lists equal.
//!
//! A session's mark ([`mark`]):
//! - a face it was given ("draw:<face>") stays;
//! - a symbol of before ("draw:rocket", the 52 drawings the faces replaced) becomes the face it was mapped to;
//! - no mark (or one no app knows): the face whose topic is a word of the session's name ("Fernly Website" -> cloud),
//!   else one picked by its id, the same on every device.

/// One face: its name, one word on what it suits (the agents' list), and the words of a session name it fits.
pub struct Face {
    pub name: &'static str,
    pub meaning: &'static str,
    pub topics: &'static [&'static str],
}

/// The faces, in the picker's order (the hue turns with the place).
pub const FACES: [Face; 24] = [
    Face { name: "blob", meaning: "general", topics: &["general", "misc", "todo", "chores"] },
    Face { name: "cloud", meaning: "web", topics: &["host", "infra", "deploy", "browser", "web", "site", "website", "frontend", "html", "css", "js", "wasm"] },
    Face { name: "flower", meaning: "polish", topics: &["garden", "polish", "detail", "details"] },
    Face { name: "drop", meaning: "release", topics: &["arrow", "connector", "bridge", "mcp", "rocket", "release", "launch", "publish", "version"] },
    Face { name: "bean", meaning: "docs", topics: &["fish", "boat", "ship", "sail", "book", "docs", "doc", "documentation", "readme", "spec", "manual", "help", "changelog", "write", "writing"] },
    Face { name: "star", meaning: "lead", topics: &["favorite", "favourite", "crown", "main", "lead", "boss"] },
    Face { name: "ghost", meaning: "mobile", topics: &["bird", "tweet", "social", "phone", "ios", "iphone", "android", "mobile", "app", "testflight", "swift"] },
    Face { name: "egg", meaning: "review", topics: &["eye", "review", "audit", "monitor", "watch", "look", "moon", "night", "sleep", "dark"] },
    Face { name: "bell", meaning: "board", topics: &["anchor", "mac", "macos", "mini", "xcode", "house", "home", "desk", "trommi", "notify", "notification", "push"] },
    Face { name: "pebble", meaning: "security", topics: &["zigzag", "cross", "cancel", "remove", "cleanup", "clean", "lock", "core", "crypto", "security", "secure", "encryption", "mls"] },
    Face { name: "heart", meaning: "care", topics: &["health", "love", "care", "support"] },
    Face { name: "mushroom", meaning: "backup", topics: &["tree", "structure", "cup", "coffee", "break", "umbrella", "backup", "shelter", "cover"] },
    Face { name: "leaf", meaning: "git", topics: &["eco", "branch", "ci", "git", "pr", "merge", "pipeline", "actions", "build", "builds"] },
    Face { name: "box", meaning: "shell", topics: &["hatch", "comb", "square", "grid", "table", "layout", "terminal", "linux", "cli", "shell", "bash", "cmd"] },
    Face { name: "peak", meaning: "speed", topics: &["bolt", "fast", "perf", "performance", "triangle", "warning", "mountain", "hill"] },
    Face { name: "sun", meaning: "idea", topics: &["burst", "idea", "ideas", "spark", "new"] },
    Face { name: "peanut", meaning: "tangle", topics: &["knot", "tangle", "refactor", "refactoring"] },
    Face { name: "onion", meaning: "urgent", topics: &["flag", "milestone", "goal", "flame", "hot", "urgent", "fire", "mic", "video", "audio", "clip", "voice", "sound", "podcast"] },
    Face { name: "cat", meaning: "bugs", topics: &["bug", "bugs", "fix", "debug", "crash"] },
    Face { name: "jelly", meaning: "sync", topics: &["waves", "stream", "sync", "network"] },
    Face { name: "acorn", meaning: "tests", topics: &["key", "auth", "login", "passkey", "keys", "flask", "test", "tests", "testing", "qa", "e2e", "lab", "fuzz"] },
    Face { name: "gem", meaning: "quality", topics: &["kite", "diamond", "quality"] },
    Face { name: "curl", meaning: "design", topics: &["spiral", "brush", "design", "designer", "brand", "style", "art", "logo", "icon", "icons"] },
    Face { name: "pill", meaning: "data", topics: &["eight", "loop", "ladder", "plan", "steps", "roadmap", "database", "db", "data", "hub", "server", "sql", "storage"] },
];

/// The 52 symbols of before and the face each became.
pub const BEFORE: [(&str, &str); 52] = [
    ("burst", "sun"), ("spiral", "curl"), ("blob", "blob"), ("flower", "flower"), ("waves", "jelly"), ("knot", "peanut"),
    ("bolt", "peak"), ("hatch", "box"), ("star", "star"), ("zigzag", "pebble"), ("eight", "pill"), ("arrow", "drop"),
    ("leaf", "leaf"), ("eye", "egg"), ("key", "acorn"), ("anchor", "bell"), ("kite", "gem"), ("comb", "box"),
    ("ladder", "pill"), ("heart", "heart"), ("moon", "egg"), ("cloud", "cloud"), ("drop", "drop"), ("flag", "onion"),
    ("house", "bell"), ("tree", "mushroom"), ("fish", "bean"), ("bird", "ghost"), ("cup", "mushroom"), ("bell", "bell"),
    ("cross", "pebble"), ("triangle", "peak"), ("square", "box"), ("diamond", "gem"), ("grid", "box"), ("mountain", "peak"),
    ("umbrella", "mushroom"), ("crown", "star"), ("flame", "onion"), ("boat", "bean"), ("browser", "cloud"),
    ("terminal", "box"), ("database", "pill"), ("phone", "ghost"), ("brush", "curl"), ("flask", "acorn"),
    ("lock", "pebble"), ("book", "bean"), ("rocket", "drop"), ("mic", "onion"), ("bug", "cat"), ("branch", "leaf"),
];

/// The face a name gives, a face's own or a symbol of before ("Rocket", "draw:rocket" -> "drop"); None for others.
pub fn face_of(given: &str) -> Option<&'static str> {
    let g = given.trim().to_lowercase();
    let bare = g.strip_prefix("draw:").unwrap_or(&g).trim();
    FACES.iter().map(|f| f.name).find(|n| *n == bare).or_else(|| BEFORE.iter().find(|(old, _)| *old == bare).map(|(_, f)| *f))
}

/// The face whose topic is a word of this name: the first such word wins.
fn by_name(name: &str) -> Option<&'static str> {
    name.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .find_map(|w| FACES.iter().find(|f| f.name == w || f.topics.contains(&w)).map(|f| f.name))
}

/// The face picked by an id: FNV-1a over its bytes, the same on every device.
fn by_id(id: &str) -> &'static str {
    let mut h: u32 = 2166136261;
    for b in id.bytes() {
        h = (h ^ u32::from(b)).wrapping_mul(16777619);
    }
    FACES[(h % FACES.len() as u32) as usize].name
}

/// A session's mark, always "draw:<face>": its icon's face, else the one its name fits, else the one its id picks.
pub fn mark(icon: &str, name: &str, id: &str) -> String {
    let face = face_of(icon).or_else(|| by_name(name)).unwrap_or_else(|| by_id(id));
    format!("draw:{face}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_unique_and_topics_fit_one_face() {
        let mut names = std::collections::HashSet::new();
        let mut topics = std::collections::HashMap::new();
        for f in &FACES {
            assert!(names.insert(f.name), "{} twice", f.name);
            assert!(!f.meaning.is_empty() && !f.meaning.contains(' '), "{}: one word", f.name);
            for t in f.topics {
                assert!(FACES.iter().all(|o| o.name != *t), "topic {t} is a face's name");
                if let Some(o) = topics.insert(*t, f.name) {
                    panic!("topic {t} for both {o} and {}", f.name);
                }
            }
        }
        for (old, face) in BEFORE {
            assert!(FACES.iter().any(|f| f.name == face), "{old} -> {face}: no such face");
        }
    }

    #[test]
    fn a_session_gets_its_face_from_its_icon_its_name_or_its_id() {
        assert_eq!(mark("draw:cloud", "Server", "a"), "draw:cloud");
        assert_eq!(mark("draw:rocket", "", "a"), "draw:drop");
        assert_eq!(mark("Rocket", "", "a"), "draw:drop");
        assert_eq!(mark("", "Fernly Website", "a"), "draw:cloud");
        assert_eq!(mark("draw:nothing", "Fernly iOS", "a"), "draw:ghost");
        let picked = mark("", "Whatever", "54ddca7af4029f0526984da1e346d833");
        assert_eq!(picked, mark("", "Other words", "54ddca7af4029f0526984da1e346d833"));
        assert!(face_of(&picked).is_some());
    }
}
