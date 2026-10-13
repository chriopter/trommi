//! The connector's text handling on the fuzzer's text: what the board says as it goes into a `<channel>` tag
//! for the agent, the name a file the human sent is stored under, invite links pasted into `connect`.
//!
//! Properties:
//! - nothing panics;
//! - board text inside a channel tag cannot close the tag or open another (no `<` before a letter, `/`, `!`
//!   or `?`), and carries no control character but line feed and tab, and no character that changes the
//!   direction of text;
//! - a file the human sent is stored right in the cache folder, never outside it, under a name of plain
//!   characters.

use serde_json::{json, Value};
use std::path::Path;
use trommi_connector::bridge::{media_type_of, safe_name};
use trommi_connector::join::{check_link, room_of_link};
use trommi_connector::server::{channel_tag, channel_text};

/// Characters no one sees or that change the direction of text: bidi marks, embeddings, overrides and isolates,
/// zero width characters, BOM, tag characters (written down here again, not taken from the connector).
fn turns_direction(c: char) -> bool {
    matches!(
        c,
        '\u{ad}' | '\u{61c}' | '\u{180e}' | '\u{200b}'..='\u{200f}' | '\u{202a}'..='\u{202e}'
            | '\u{2060}'..='\u{206f}' | '\u{feff}' | '\u{fff9}'..='\u{fffb}' | '\u{e0000}'..='\u{e007f}'
    )
}

fn clean(what: &str, text: &str) {
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        assert!(
            c == '\n' || c == '\t' || !c.is_control(),
            "{what}: a control character {c:?} in {text:?}"
        );
        assert!(
            !turns_direction(c),
            "{what}: an invisible or direction mark {c:?} in {text:?}"
        );
        if c == '<' {
            let next = chars.peek().copied();
            assert!(
                !next.is_some_and(|n| n.is_ascii_alphabetic() || matches!(n, '/' | '!' | '?')),
                "{what}: a tag opens in {text:?}"
            );
        }
    }
}

pub fn run(data: &[u8]) {
    let text = String::from_utf8_lossy(data);
    let text = text.as_ref();

    clean("channel text", &channel_text(text));
    let (key_part, content) = text.split_at(
        text.char_indices()
            .nth(text.chars().count() / 3)
            .map_or(0, |(i, _)| i),
    );
    let tag = channel_tag(&json!({
        "content": content,
        "meta": { "kind": key_part, "card_id": Value::from(content.len()), "title": [key_part] },
    }));
    let mut rest = tag
        .strip_prefix("<channel source=\"board\"")
        .expect("the tag opens");
    // the head: ` key="value"` pairs, a value without `"` (it is written as &quot;), then `>` and a line feed
    while let Some(attribute) = rest.strip_prefix(' ') {
        let (key, after) = attribute.split_once("=\"").expect("key=\"");
        assert!(
            key.chars().all(|c| c.is_ascii_lowercase() || c == '_'),
            "a key the board chose: {key:?}"
        );
        let (value, after) = after.split_once('"').expect("a closing quote");
        clean("channel meta", value);
        rest = after;
    }
    let body = rest
        .strip_prefix(">\n")
        .and_then(|b| b.strip_suffix("\n</channel>"))
        .expect("one channel tag");
    assert!(
        !body.contains("</channel"),
        "the board text closed the tag: {tag:?}"
    );
    clean("channel content", body);

    let name = safe_name(text);
    assert!(
        !name.is_empty() && name.chars().count() <= 100,
        "file name {name:?}"
    );
    assert!(
        name.chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-'))
            && !name.starts_with('.'),
        "file name {name:?} of {text:?}"
    );
    let cache = Path::new("/cache/dir");
    let file = cache.join(format!("AAAAAAAAAAAAAAAAAAAAAA-{name}"));
    assert_eq!(
        file.parent(),
        Some(cache),
        "a file the human sent lands outside the cache: {file:?}"
    );
    let _ = media_type_of(text);

    let _ = room_of_link(text);
    let _ = check_link(text, 1_800_000_000_000);
}
