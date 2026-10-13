//! The shells' freeze: the app's logic is written once, here (core/client/README.md, "What a shell may do"). The
//! folders where each app still keeps logic of its own may only shrink until their stream deletes them: no new file,
//! and no file longer than it was when the freeze began. A change that must grow one (an urgent fix, a new shell
//! executor file) updates `tests/shells.json` in the same pull request, where review sees it:
//!
//! ```text
//! TROMMI_SHELLS_WRITE=1 cargo test -p trommi-client-core --test shells
//! ```

use std::collections::BTreeMap;
use std::path::Path;

/// The folders of logic each shell still holds, and the file kinds counted there.
const FROZEN: &[(&str, &str)] = &[
    ("app/web/core", ".ts"),
    ("app/ios/TrommiClient/Sources/TrommiClient", ".swift"),
    ("app/linux/src/engine", ".rs"),
];

fn now(repo: &Path) -> BTreeMap<String, usize> {
    let mut files = BTreeMap::new();
    for (folder, kind) in FROZEN {
        let Ok(entries) = std::fs::read_dir(repo.join(folder)) else { continue };
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.ends_with(kind) && !name.ends_with(".d.ts") {
                let lines = std::fs::read_to_string(entry.path()).map(|t| t.lines().count()).unwrap_or(0);
                files.insert(format!("{folder}/{name}"), lines);
            }
        }
    }
    files
}

#[test]
fn the_shells_logic_only_shrinks() {
    let here = Path::new(env!("CARGO_MANIFEST_DIR"));
    let repo = here.join("../..");
    let list = here.join("tests/shells.json");
    let found = now(&repo);
    if std::env::var_os("TROMMI_SHELLS_WRITE").is_some() {
        std::fs::write(&list, serde_json::to_string_pretty(&found).unwrap() + "\n").unwrap();
        return;
    }
    let frozen: BTreeMap<String, usize> = serde_json::from_slice(&std::fs::read(&list).unwrap()).unwrap();
    let mut problems = Vec::new();
    for (file, lines) in &found {
        match frozen.get(file) {
            None => problems.push(format!("{file} is new: app logic belongs in core/client")),
            Some(max) if lines > max => problems.push(format!(
                "{file} grew from {max} to {lines} lines: app logic belongs in core/client"
            )),
            _ => {}
        }
    }
    assert!(problems.is_empty(), "{}\n(an agreed exception: TROMMI_SHELLS_WRITE=1, see tests/shells.rs)", problems.join("\n"));
}
