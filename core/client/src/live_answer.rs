//! The answer of an agent's running turn as it stands (spec/v1.md 7.5): the connector sends it relayed, never stored,
//! once a second while the turn runs; the person's devices show it as the agent's growing answer until its message
//! comes. Every preview weighs the same ([`PREVIEW_BYTES`]), so its padded size says nothing about the answer, and
//! it is sent at the same pace whether the words changed or not: the hub learns no more than the agent's link report
//! already tells it (that the agent works).
use serde_json::{json, Value};

/// The JSON of every preview, padded to this many bytes: with its message framing and MLS's own fields it always
/// lands in the 8 192-byte bucket (7.0).
pub const PREVIEW_BYTES: usize = 7_680;
/// The most of the answer a preview carries: its end, cut at a character.
pub const TEXT_MAX: usize = 7_000;
/// How often the connector sends one while a turn runs.
pub const EVERY_MS: u64 = 1_000;
/// A preview older than this is not shown (its turn ended without its message reaching this device yet, or the agent
/// went).
pub const SHOWN_MS: u64 = 10_000;

/// The end of `text` that a preview carries: at most [`TEXT_MAX`] bytes, cut at a character.
pub fn tail(text: &str) -> &str {
    if text.len() <= TEXT_MAX {
        return text;
    }
    let mut from = text.len() - TEXT_MAX;
    while !text.is_char_boundary(from) {
        from += 1;
    }
    &text[from..]
}

/// A preview's bytes: `{"text": …, "pad": "   …"}`, exactly [`PREVIEW_BYTES`] long. The text is the answer's end; a
/// text whose JSON escapes make it longer is cut further at its front.
pub fn encode(text: &str) -> Vec<u8> {
    let mut words = tail(text);
    loop {
        let bare = json!({ "text": words, "pad": "" }).to_string();
        if bare.len() <= PREVIEW_BYTES {
            let pad = " ".repeat(PREVIEW_BYTES - bare.len());
            return json!({ "text": words, "pad": pad }).to_string().into_bytes();
        }
        let mut from = (bare.len() - PREVIEW_BYTES).max(words.len() / 10);
        while from < words.len() && !words.is_char_boundary(from) {
            from += 1;
        }
        words = &words[from.min(words.len())..];
    }
}

/// The words of a preview; none for one that is not one as 7.5 writes it: exactly [`PREVIEW_BYTES`], `text` at most
/// [`TEXT_MAX`] bytes, `pad` spaces only, nothing else.
pub fn decode(bytes: &[u8]) -> Option<String> {
    if bytes.len() != PREVIEW_BYTES {
        return None;
    }
    let v: Value = serde_json::from_slice(bytes).ok()?;
    let o = v.as_object()?;
    let text = o.get("text")?.as_str()?;
    let pad = o.get("pad")?.as_str()?;
    (o.len() == 2 && text.len() <= TEXT_MAX && pad.bytes().all(|b| b == b' ')).then(|| text.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_preview_weighs_the_same() {
        for text in ["", "Guten Tag", &"ä".repeat(5_000), &"x \"quoted\" \\ ".repeat(2_000)] {
            let bytes = encode(text);
            assert_eq!(bytes.len(), PREVIEW_BYTES, "{}", text.len());
            let back = decode(&bytes).unwrap();
            assert!(text.ends_with(&back) && back.len() <= TEXT_MAX);
        }
        assert_eq!(decode(&encode("Hallo")).as_deref(), Some("Hallo"));
        assert_eq!(decode(b"{\"step\":1}"), None);
        // only exactly as 7.5 writes it
        let mut short = encode("Hallo");
        short.pop();
        assert_eq!(decode(&short), None);
        let odd = format!("{{\"text\":\"x\",\"pad\":\"{}\"}}", "y".repeat(PREVIEW_BYTES - 21));
        assert_eq!((odd.len(), decode(odd.as_bytes())), (PREVIEW_BYTES, None));
    }
}
