//! Every text the agent reads, from connector/prompt.md (compiled in), and the tools' schemas from tools.json (both
//! edited by hand; build.rs checks them).
use regex::Regex;
use serde_json::{Map, Value};
use std::collections::HashMap;
use std::sync::OnceLock;

pub const PROMPT_MD: &str = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/prompt.md"));
pub const TOOLS_JSON: &str = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/tools.json"));

/// The compiled MCP contract, across all host and mirror variants. Runtime connection status and
/// release versions are deliberately absent: neither changes the tools or initialization instructions.
pub fn contract_fingerprint() -> String {
    fingerprint(PROMPT_MD, tools_text())
}

fn fingerprint(md: &str, tools: &str) -> String {
    let sections: std::collections::BTreeMap<_, _> = parse_prompt(md).into_iter().collect();
    let tools: Value = serde_json::from_str(tools).expect("tools.json");
    let mut contract = serde_json::json!({ "instructions": sections, "tools": tools });
    contract.sort_all_objects();
    crate::util::hex(&crate::util::sha256(contract.to_string().as_bytes()))
}

/// prompt.md as { '# Heading' | '## tool': text }: a section's lines and paragraphs read as one paragraph.
pub fn parse_prompt(md: &str) -> HashMap<String, String> {
    let h = Regex::new(r"^(##?) +(.+?)\s*$").unwrap();
    let mut out: HashMap<String, String> = HashMap::new();
    let mut at: Option<String> = None;
    for line in md.split('\n') {
        let line = line.strip_suffix('\r').unwrap_or(line);
        if let Some(c) = h.captures(line) {
            let k = format!("{} {}", &c[1], &c[2]);
            out.insert(k.clone(), String::new());
            at = Some(k);
        } else if let Some(k) = &at {
            let v = out.get_mut(k).unwrap();
            v.push(' ');
            v.push_str(line);
        }
    }
    let ws = Regex::new(r"\s+").unwrap();
    for v in out.values_mut() {
        *v = ws.replace_all(v, " ").trim().to_string();
    }
    out
}

pub fn prompt() -> &'static HashMap<String, String> {
    static P: OnceLock<HashMap<String, String>> = OnceLock::new();
    P.get_or_init(|| parse_prompt(PROMPT_MD))
}
pub fn description(tool: &str) -> String {
    prompt()
        .get(&format!("## {tool}"))
        .cloned()
        .unwrap_or_default()
}
/// Whether this session's terminal is mirrored to the board: the plugin's hooks do it (README "The terminal mirror").
/// Under Codex: its hooks.json holds the connector's hooks and they are trusted (`setup codex`).
pub fn terminal_mirrored() -> bool {
    if !crate::mirror::enabled() {
        return false;
    }
    if crate::host::is_codex() {
        static CODEX: OnceLock<bool> = OnceLock::new();
        return *CODEX.get_or_init(crate::setup::codex_mirror_hooks);
    }
    std::env::var_os("CLAUDE_PLUGIN_ROOT").is_some()
}
/// The server's instructions; `<connector>` stands for the connector's path. `<mirror>` is filled in here: how the
/// agent's words reach the human, with the terminal mirror or without it.
pub fn instructions() -> String {
    // (Claude Code: the plugin's hooks; Codex: the hooks `setup codex` put into its hooks.json, once trusted)
    let how = if crate::host::is_chat() {
        "# Through the chat bridge"
    } else if terminal_mirrored() {
        "# With the terminal mirror"
    } else {
        "# Without the terminal mirror"
    };
    let host = match crate::host::preset().id {
        "codex" => "# In Codex",
        "hermes" => "# In Hermes",
        _ => "# In Claude Code",
    };
    prompt()
        .get("# Instructions")
        .cloned()
        .unwrap_or_default()
        .replacen(
            "<mirror>",
            prompt().get(how).map(String::as_str).unwrap_or(""),
            1,
        )
        .replacen(
            "<host>",
            prompt().get(host).map(String::as_str).unwrap_or(""),
            1,
        )
}
/// The inbox tool as Claude Code names it: the plugin's server, or a .mcp.json server "trommi".
pub fn inbox_tool_name() -> &'static str {
    if std::env::var_os("CLAUDE_PLUGIN_ROOT").is_some() {
        "mcp__plugin_trommi_trommi__inbox"
    } else {
        "mcp__trommi__inbox"
    }
}
/// What goes in front of the instructions in a session without channel events.
pub fn monitor_note() -> String {
    prompt()
        .get("# Without channel events")
        .cloned()
        .unwrap_or_default()
        .replacen("<inbox>", inbox_tool_name(), 1)
}

/// tools.json with the list of drawings (the app core's faces) put where it says {drawings}.
fn tools_text() -> &'static str {
    static T: OnceLock<String> = OnceLock::new();
    T.get_or_init(|| TOOLS_JSON.replace("{drawings}", &crate::drawings::listing()))
}
fn tools_doc() -> &'static Value {
    static T: OnceLock<Value> = OnceLock::new();
    T.get_or_init(|| serde_json::from_str(tools_text()).expect("tools.json"))
}
fn with_description(t: &Value) -> Value {
    let mut m: Map<String, Value> = t.as_object().cloned().unwrap_or_default();
    let name = m["name"].as_str().unwrap_or("").to_string();
    m.insert("description".into(), Value::String(description(&name)));
    let v = Value::Object(m);
    // BOARD_MAX_HTML_KB other than the default: the texts say so
    let kb = crate::html::html_max_kb();
    if kb != 200 {
        let s = serde_json::to_string(&v)
            .unwrap()
            .replace("At most 200 KB", &format!("At most {kb} KB"));
        return serde_json::from_str(&s).unwrap();
    }
    v
}
/// JSON text without the white space between its tokens. The order of every object's keys stays as written.
fn minified(json: &str) -> String {
    let mut out = String::with_capacity(json.len());
    let (mut in_string, mut escaped) = (false, false);
    for c in json.chars() {
        if in_string {
            out.push(c);
            if escaped {
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if c == '"' {
                in_string = false;
            }
        } else if c == '"' {
            in_string = true;
            out.push(c);
        } else if !c.is_whitespace() {
            out.push(c);
        }
    }
    out
}

#[derive(serde::Deserialize)]
struct RawTools<'a> {
    #[serde(borrow)]
    tools: Vec<&'a serde_json::value::RawValue>,
    #[serde(borrow)]
    reload: &'a serde_json::value::RawValue,
    #[serde(borrow)]
    inbox: &'a serde_json::value::RawValue,
    #[serde(borrow)]
    connect: &'a serde_json::value::RawValue,
}

/// One tool as it is written in tools.json, on one line, with its description from prompt.md in front. The
/// keys and the parameters keep the order they were written in: that is the order the agent reads them in.
fn raw_tool(raw: &serde_json::value::RawValue) -> String {
    let name = serde_json::from_str::<Value>(raw.get())
        .ok()
        .and_then(|tool| tool.get("name").and_then(|n| n.as_str().map(String::from)))
        .unwrap_or_default();
    let text = minified(raw.get());
    let kb = crate::html::html_max_kb();
    let text = if kb == 200 {
        text
    } else {
        text.replace("At most 200 KB", &format!("At most {kb} KB"))
    };
    let description = Value::String(description(&name)).to_string();
    format!(
        "{{\"description\":{description},{}",
        text.strip_prefix('{').unwrap_or(&text)
    )
}

/// The answer to `tools/list` as JSON text: every tool, the reload tool, and with `inbox` the inbox tool.
pub fn tools_list_json(inbox: bool) -> String {
    let Ok(doc) = serde_json::from_str::<RawTools>(tools_text()) else {
        return "{\"tools\":[]}".into();
    };
    let mut tools: Vec<String> = doc.tools.iter().map(|raw| raw_tool(raw)).collect();
    tools.push(raw_tool(doc.reload));
    if inbox {
        tools.push(raw_tool(doc.inbox));
    }
    format!("{{\"tools\":[{}]}}", tools.join(","))
}

/// The same with the connect tool in front: for a client that never asks for the list again (Codex), and for a
/// session activated explicitly on a computer.
pub fn tools_list_json_with_connect(inbox: bool, note: Option<&str>) -> String {
    let all = tools_list_json(inbox);
    let rest = all.strip_prefix("{\"tools\":[").unwrap_or("]}");
    format!("{{\"tools\":[{},{rest}", connect_tool(note))
}

/// The answer to `tools/list` of a session whose folder is in no room yet: the connect tool alone.
pub fn connect_list_json(note: Option<&str>) -> String {
    format!("{{\"tools\":[{}]}}", connect_tool(note))
}

/// The connect tool as JSON text; with an optional connection status `note` in front of its
/// description.
fn connect_tool(note: Option<&str>) -> String {
    let Ok(doc) = serde_json::from_str::<RawTools>(tools_text()) else {
        return "{}".into();
    };
    let tool = raw_tool(doc.connect);
    let Some(note) = note.filter(|n| !n.is_empty()) else {
        return tool;
    };
    let Ok(mut v) = serde_json::from_str::<Value>(&tool) else {
        return tool;
    };
    let said = format!(
        "{}. {}",
        note.trim_end_matches('.'),
        v["description"].as_str().unwrap_or("")
    );
    v["description"] = Value::String(said);
    v.to_string()
}

/// The tools (TOOLS), each with its description.
pub fn tools() -> Vec<Value> {
    tools_doc()["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(with_description)
        .collect()
}
pub fn reload_tool() -> Value {
    with_description(&tools_doc()["reload"])
}
pub fn inbox_tool() -> Value {
    with_description(&tools_doc()["inbox"])
}
pub fn examples() -> &'static Value {
    &tools_doc()["examples"]
}

#[cfg(test)]
mod contract_tests {
    use super::*;

    #[test]
    fn cached_tools_and_instructions_require_a_new_contract() {
        let original = contract_fingerprint();
        assert_eq!(original.len(), 64);
        let mut tools: Value = serde_json::from_str(tools_text()).unwrap();
        tools["connect"]["inputSchema"]["required"] = serde_json::json!(["link"]);
        assert_ne!(original, fingerprint(PROMPT_MD, &tools.to_string()));
        for heading in [
            "# Instructions",
            "# In Codex",
            "# In Claude Code",
            "# In Hermes",
            "# Through the chat bridge",
            "# With the terminal mirror",
            "# Without the terminal mirror",
            "## connect",
        ] {
            assert!(PROMPT_MD.contains(heading));
            let changed = PROMPT_MD.replace(heading, &format!("{heading}\nChanged contract."));
            assert_ne!(original, fingerprint(&changed, tools_text()), "{heading}");
        }
    }

    #[test]
    fn formatting_and_object_order_do_not_require_a_restart() {
        assert_eq!(
            fingerprint(
                "# A\nOne two.\n# B\nThree.\n",
                r#"{"b":{"d":2,"c":1},"a":0}"#
            ),
            fingerprint(
                "# B\r\n Three.\r\n# A\r\nOne   two.\r\n",
                r#"{ "a":0, "b": { "c":1, "d":2 } }"#
            )
        );
    }
}
