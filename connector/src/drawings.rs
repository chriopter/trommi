//! The faces a session can wear as its symbol: the app core's list (`trommi_client_core::faces`), the one list the
//! agent's tool descriptions and the check of a given icon come from. The apps hold the same names in the same order
//! (app/web/public/ui.mjs `FACES`, app/ios/.../Pen.swift `drawings`); a test keeps them equal.
use trommi_client_core::faces::{face_of, FACES};

/// "blob (general), cloud (web), …": the names with a word each, for the tools' `icon` description.
pub fn listing() -> String {
    FACES
        .iter()
        .map(|f| format!("{} ({})", f.name, f.meaning))
        .collect::<Vec<_>>()
        .join(", ")
}

/// The mark the board stores for a face: "draw:cloud". Accepts "cloud", "Cloud" and "draw:cloud", and a symbol of
/// before ("rocket" -> "draw:drop"). An unknown name is an error that lists the valid ones.
pub fn mark_of(given: &str) -> Result<String, String> {
    match face_of(given) {
        Some(face) => Ok(format!("draw:{face}")),
        None => Err(format!(
            "no face named \"{given}\". Pass one of these names (no prefix needed): {}",
            listing()
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_given_name_gets_its_prefix_and_an_unknown_one_lists_the_valid() {
        assert_eq!(mark_of("cloud").unwrap(), "draw:cloud");
        assert_eq!(mark_of(" Draw:Cloud ").unwrap(), "draw:cloud");
        assert_eq!(mark_of("rocket").unwrap(), "draw:drop");
        let e = mark_of("spaceship").unwrap_err();
        assert!(e.contains("spaceship") && e.contains("drop (release)") && e.contains("blob (general)"), "{e}");
    }

    /// The web and the iOS app hold the same faces, in the same order, as the app core.
    #[test]
    fn web_and_ios_hold_the_same_list() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
        let want: Vec<String> = FACES.iter().map(|f| f.name.to_string()).collect();

        let ui = std::fs::read_to_string(root.join("app/web/public/ui.mjs")).unwrap();
        let faces = ui.split("const FACES = {").nth(1).unwrap().split("\n}").next().unwrap();
        let web: Vec<String> = faces.lines().filter(|l| l.starts_with("  ") && !l.starts_with("   ") && !l.trim_start().starts_with("//"))
            .filter_map(|l| l.trim().split(':').next().map(String::from)).collect();
        assert_eq!(web, want, "app/web/public/ui.mjs");

        let pen = std::fs::read_to_string(root.join("app/ios/TrommiApp/Sources/TrommiApp/Pen.swift")).unwrap();
        let drawings = pen.split("static let drawings = [").nth(1).unwrap().split(']').next().unwrap().to_string();
        let ios: Vec<String> = drawings.split('"').skip(1).step_by(2).map(String::from).collect();
        assert_eq!(ios, want, "Pen.swift");

        let public: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(root.join("app/web/public/drawings.json")).unwrap()).unwrap();
        let public: Vec<String> = public.as_array().unwrap().iter().map(|d| d["name"].as_str().unwrap().to_string()).collect();
        assert_eq!(public, want, "app/web/public/drawings.json");
    }
}
