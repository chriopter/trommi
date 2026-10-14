//! Links and layouts in an agent's words, as the screens show them (the web's `ui.mjs` `linkInfo`, `withoutLayouts`,
//! `tidyLinks`, `plain`):
//! - a link to an Artifact of the board (`/artifact/<id>#<key>`, also on a full address) is named by the Artifact's
//!   title, else by its kind ("Page", "Picture", "Video", "Audio", "File"), else "published link";
//! - any other link, in one line of plain words, is its address without scheme, "www.", query and fragment, cut to
//!   34 signs;
//! - an agent's own layout (a block fenced as html) and a table stand in no line of plain words: they are drawn on
//!   the card's or the conversation's page.
//!
//! On a page (markdown, drawn by the shell) only the Artifact links change: a bare one becomes `[title](address)`,
//! so that every shell's markdown names it by its title. Code blocks are left as written.

use crate::model::Model;
use serde_json::Value;
use std::collections::HashMap;

/// The board's Artifacts that are not closed, by id: their title and kind.
#[derive(Clone, Debug, Default)]
pub struct Assets(HashMap<String, (String, &'static str)>);

fn kind_of(media_type: &str) -> &'static str {
    if media_type == "text/html" {
        "html"
    } else if media_type.starts_with("image/") {
        "image"
    } else if media_type.starts_with("video/") {
        "video"
    } else if media_type.starts_with("audio/") {
        "audio"
    } else {
        "file"
    }
}

impl Assets {
    pub fn of(m: &Model) -> Self {
        Assets(
            m.published
                .values()
                .filter(|p| p.object_state != "closed")
                .map(|p| {
                    let media = p.attachments.get(0).and_then(|a| a.get("media_type")).and_then(Value::as_str).unwrap_or("");
                    (p.object_id.clone(), (p.title.clone(), kind_of(media)))
                })
                .collect(),
        )
    }
    /// One Artifact, for tests.
    pub fn with(mut self, id: &str, title: &str, media_type: &str) -> Self {
        self.0.insert(id.into(), (title.into(), kind_of(media_type)));
        self
    }
}

fn id_sign(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_' || c == '-'
}

/// What an address points at: an Artifact of the board (its name), or anything else (its short words).
enum Info {
    Asset(String),
    Other(String),
}

/// The Artifact's id in an address, when it is one: `(https://host)?/(artifact|a)/<16 to 64 signs>#<43 signs>`.
fn asset_id(url: &str) -> Option<(&str, bool)> {
    let (origin, path) = match url.strip_prefix("https://").or_else(|| url.strip_prefix("http://")) {
        Some(rest) => {
            let slash = rest.find('/')?;
            if slash == 0 || rest[..slash].chars().any(char::is_whitespace) {
                return None;
            }
            (true, &rest[slash..])
        }
        None => (false, url),
    };
    let rest = path.strip_prefix("/artifact/").or_else(|| path.strip_prefix("/a/"))?;
    let (id, key) = rest.split_once('#')?;
    let good = (16..=64).contains(&id.len()) && id.chars().all(id_sign) && key.len() == 43 && key.chars().all(id_sign);
    good.then_some((id, origin))
}

fn info(url: &str, assets: &Assets) -> Info {
    if let Some((id, origin)) = asset_id(url) {
        let record = assets.0.get(id);
        if record.is_some() || !origin {
            let (title, kind) = record.map(|(t, k)| (t.as_str(), *k)).unwrap_or(("", ""));
            let name = if !title.is_empty() {
                title.to_string()
            } else {
                match kind {
                    "html" => "Page",
                    "image" => "Picture",
                    "video" => "Video",
                    "audio" => "Audio",
                    "file" => "File",
                    _ => "published link",
                }
                .to_string()
            };
            return Info::Asset(name);
        }
    }
    let mut text = url.strip_prefix("https://").or_else(|| url.strip_prefix("http://")).unwrap_or(url);
    text = text.strip_prefix("www.").unwrap_or(text);
    if let Some(at) = text.find(['?', '#']) {
        text = &text[..at];
    }
    let text = text.strip_suffix('/').unwrap_or(text);
    // (JavaScript counts UTF-16 units; an address is ASCII but for rare cases)
    if text.encode_utf16().count() > 34 {
        let mut units = 0;
        let cut: String = text.chars().take_while(|c| { units += c.len_utf16(); units <= 33 }).collect();
        Info::Other(format!("{cut}…"))
    } else {
        Info::Other(text.to_string())
    }
}

/// Where an address starting at `at` ends: `https?://` then signs that are no space, `<`, `>`, `)` or a backtick.
fn url_end(text: &str, at: usize) -> Option<usize> {
    let rest = &text[at..];
    let head = if rest.starts_with("https://") {
        8
    } else if rest.starts_with("http://") {
        7
    } else {
        return None;
    };
    let body = rest[head..].find(|c: char| c.is_whitespace() || matches!(c, '<' | '>' | ')' | '`')).unwrap_or(rest.len() - head);
    (body > 0).then_some(at + head + body)
}

/// The text without an agent's layouts: blocks fenced as html, table rows and the rule under a table's head.
pub fn without_layouts(text: &str) -> String {
    let text = without_html_fences(text);
    let lines: Vec<&str> = text.split('\n').collect();
    let row = |l: &str| {
        let t = l.trim();
        t.len() >= 2 && t.starts_with('|') && t.ends_with('|')
    };
    let rule = |l: &str| {
        let t = l.trim();
        !t.is_empty() && t.contains('-') && t.chars().all(|c| matches!(c, '|' | ':' | '-') || c.is_whitespace())
    };
    lines
        .iter()
        .enumerate()
        .filter(|(i, l)| !(row(l) || (rule(l) && i.checked_sub(1).is_some_and(|p| lines[p].contains('|')))))
        .map(|(_, l)| *l)
        .collect::<Vec<_>>()
        .join("\n")
}

/// Blocks fenced as html (```` ```html ```` to the next fence, any case) out, each a space.
fn without_html_fences(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    loop {
        let lower = rest.to_ascii_lowercase();
        let Some(open) = lower.find("```html") else { break };
        // (the fence's line, then its body up to the closing fence)
        let Some(nl) = rest[open..].find('\n') else { break };
        let body = open + nl + 1;
        let Some(close) = rest[body..].find("```") else { break };
        out.push_str(&rest[..open]);
        out.push(' ');
        rest = &rest[body + close + 3..];
    }
    out.push_str(rest);
    out
}

/// The text without layouts, its links as one line names them: an Artifact as `[title]`, another address short.
pub fn tidy_links(text: &str, assets: &Assets) -> String {
    let text = without_layouts(text);
    let mut out = String::with_capacity(text.len());
    let mut last = 0;
    let mut at = 0;
    while let Some(off) = text[at..].find("http") {
        let start = at + off;
        let Some(end) = url_end(&text, start) else {
            at = start + 4;
            continue;
        };
        // (a backtick on either side goes with it)
        let from = if text[..start].ends_with('`') { start - 1 } else { start };
        let to = if text[end..].starts_with('`') { end + 1 } else { end };
        out.push_str(&text[last..from]);
        match info(&text[start..end], assets) {
            Info::Asset(name) => {
                out.push('[');
                out.push_str(&name);
                out.push(']');
            }
            Info::Other(short) => out.push_str(&short),
        }
        last = to;
        at = to;
    }
    out.push_str(&text[last..]);
    out
}

/// A text as one line of plain words: no fences, no layouts, no markup signs, links as what they are.
pub fn plain(text: &str, assets: &Assets) -> String {
    let mut out = String::new();
    let mut rest = text;
    while let Some(open) = rest.find("```") {
        out.push_str(&rest[..open]);
        match rest[open + 3..].find("```") {
            Some(close) => {
                out.push(' ');
                rest = &rest[open + 3 + close + 3..];
            }
            None => {
                rest = &rest[open..];
                break;
            }
        }
    }
    out.push_str(rest);
    let out = tidy_links(&out, assets);
    // (__underlined__ words keep their words: the signs go with the others below, and a lone _ stays)
    let out = out.replace("__", "");
    let out: String = out.chars().filter(|c| !matches!(c, '*' | '`' | '#')).collect();
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Markdown with its bare Artifact links named by their titles (`[title](address)`); code blocks, code spans that
/// are no address and links that have words already are left as written.
pub fn titled(text: &str, assets: &Assets) -> String {
    if !text.contains("/a") {
        return text.to_string();
    }
    let mut out = String::with_capacity(text.len() + 32);
    let mut fenced = false;
    for (n, line) in text.split('\n').enumerate() {
        if n > 0 {
            out.push('\n');
        }
        if line.trim_start().starts_with("```") {
            fenced = !fenced;
            out.push_str(line);
            continue;
        }
        if fenced {
            out.push_str(line);
            continue;
        }
        let mut last = 0;
        let mut at = 0;
        while let Some(off) = line[at..].find("http") {
            let start = at + off;
            let Some(end) = url_end(line, start) else {
                at = start + 4;
                continue;
            };
            at = end;
            // (the target of a link with words, `[words](address)`, keeps its words)
            if line[..start].ends_with("](") {
                continue;
            }
            let Info::Asset(name) = info(&line[start..end], assets) else { continue };
            let ticked = line[..start].ends_with('`') && line[end..].starts_with('`');
            let (from, to) = if ticked { (start - 1, end + 1) } else { (start, end) };
            out.push_str(&line[last..from]);
            let name = name.replace(['[', ']'], "");
            let name = if name == "published link" { "Published link".to_string() } else { name };
            out.push_str(&format!("[{name}]({})", &line[start..end]));
            last = to;
            at = to;
        }
        out.push_str(&line[last..]);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const KEY: &str = "abcdefghijABCDEFGHIJ0123456789_-abcdefghijk";

    fn assets() -> Assets {
        Assets::default().with("0123456789abcdef0123", "Launch plan", "text/html").with("ffffeeeeddddcccc", "", "image/png")
    }

    #[test]
    fn links_in_one_line_are_named() {
        let a = assets();
        assert_eq!(KEY.len(), 43);
        let key = KEY;
        let page = format!("https://app.trommi.com/artifact/0123456789abcdef0123#{key}");
        let pic = format!("https://app.trommi.com/a/ffffeeeeddddcccc#{key}");
        let unknown = format!("https://app.trommi.com/artifact/9999999999999999#{key}");
        let text = format!("See {page} and `{pic}`, also {unknown} and https://www.example.com/docs/guide/?x=1#top.");
        assert_eq!(
            plain(&text, &a),
            "See [Launch plan] and [Picture], also app.trommi.com/artifact/999999999… and example.com/docs/guide"
        );
        // (the fragment goes, and the full stop that followed it with it, as on the web)
        // an address too long is cut to 34 signs
        assert_eq!(plain("https://github.com/trommi/trommi/pull/187/files", &a), "github.com/trommi/trommi/pull/187…");
    }

    #[test]
    fn layouts_stand_in_no_line() {
        let text = "Pick one.\n```HTML\n<div>tiles</div>\n```\n| a | b |\n|---|---|\n| 1 | 2 |\nThen ship - soon.\n- a list stays";
        assert_eq!(plain(text, &Assets::default()), "Pick one. Then ship - soon. - a list stays");
        // a rule without a table above it stays
        assert_eq!(without_layouts("one\n---\ntwo"), "one\n---\ntwo");
    }

    #[test]
    fn a_page_names_its_artifact_links() {
        let a = assets();
        let page = format!("https://app.trommi.com/artifact/0123456789abcdef0123#{KEY}");
        let text = format!("The plan: {page}\n[my words]({page})\n```\n{page}\n```\n`{page}` and https://example.com");
        assert_eq!(
            titled(&text, &a),
            format!("The plan: [Launch plan]({page})\n[my words]({page})\n```\n{page}\n```\n[Launch plan]({page}) and https://example.com")
        );
    }
}
