//! The rows of one table on the admin page: `/tables/<name>` lists them, 50 to a page (newest first, or sorted by
//! one column, or only those where one column is one value), `/tables/<name>/<key>` shows one row with every
//! value in full. Read-only, in one read transaction, with one small script of the hub's own (`admin.js`: a double-click shows an e-mail address).
//!
//! What a value shows is set per column (`show`): only the columns named in `FULL` are shown in full; what lies
//! sealed (`admin_view::SEALED`) by its length and first bytes (it is unreadable anyway); the e-mail address
//! blurred as on the accounts page (a double-click shows it); everything else, the credentials of `SECRET` and any column no list names, as
//! `•••• (secret, n bytes)`. A secret column can be neither filtered nor sorted by (that would answer questions
//! about its values). Table and column names come only from the schema list, values only as bound parameters.
//!
//! A row is named by its key: the rowid, or for a table `WITHOUT ROWID` its primary key, written as the values
//! joined by `.` (a number as it is, bytes as `x` and hex, text as `t` and hex). Pages go on from the key of the
//! last row shown (`a=`, after) or back from the first (`b=`, before): keyset paging, so a page holds still while
//! rows are written elsewhere.

use rusqlite::types::Value;
use rusqlite::Connection;

use crate::admin::{esc, EYE, LOCK};
use crate::admin_view::{columns, framed, is_time, blurred_email, side, table_named, when_exactly, SEALED, TABLES};
use crate::app::App;
use crate::error::Refused;
use crate::util::{hex, unhex};

/// rows on one page
pub const PAGE: usize = 50;
/// characters of a text shown in the list (the row's own page shows all)
const TEXT_SHOWN: usize = 60;
/// bytes shown of a byte string in the list, and of a sealed one anywhere
const BYTES_SHOWN: usize = 16;
/// bytes shown in hex on a row's own page
const BYTES_IN_FULL: usize = 4096;

/// How a column's values appear.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Show {
    /// as it is
    Full,
    /// in full but blurred until double-clicked, as on the accounts page
    Email,
    /// what the hub cannot read: its length and first bytes
    Sealed,
    /// a credential, or a column no list names: only that it is there and its length
    Secret,
}

/// The columns shown in full, table by table: ids, numbers, times, words of a short fixed list, public keys,
/// signed public MLS state, the hub's own keyed hashes of addresses, hashes of public data. A column that is not
/// here (nor sealed, nor the e-mail address) is shown as a secret.
pub const FULL: &[(&str, &[&str])] = &[
    ("accounts", &["account_id", "created_at", "updated_at", "revision", "kit_form", "user_handle"]),
    ("passkeys", &["credential_id", "account_id", "public_key", "algorithm", "sign_count", "transports", "created_at", "last_used_at"]),
    ("account_rooms", &["account_id", "room_id", "position"]),
    ("account_sources", &["account_id", "source", "last_at"]),
    ("login_sources", &["account", "source", "failures", "next_at", "checking", "over"]),
    ("login_accounts", &["account", "budget_start", "budget_used", "next_turn"]),
    ("login_turns", &["account", "source", "turn", "good_until"]),
    ("rooms", &["room_id", "founded_at", "change", "file_bytes", "file_count", "register_count", "void_count", "group_count", "device_count", "helper_count", "epoch", "room_state", "recovery_signature_key", "recovery_hpke_key"]),
    ("devices", &["room_id", "device", "role", "added_epoch", "removed_epoch"]),
    ("requests", &["id", "room_id", "device", "kind", "group_id", "committer", "key_package", "n", "at"]),
    ("agent_leases", &["room_id", "device", "process", "generation", "expires_at", "hears", "working", "last_call_at", "lost_at"]),
    ("groups", &["group_id", "room_id", "kind", "session_id", "parent", "founder", "epoch", "room_epoch", "epoch_at", "live", "archived_at", "log_n", "log_kept_from", "founded_change", "state"]),
    ("group_members", &["group_id", "device", "added_epoch", "removed_epoch", "cut_seq", "cut_hash"]),
    ("group_log", &["group_id", "n", "room_id", "epoch", "kind", "sender", "at", "change", "digest"]),
    ("group_infos", &["group_id", "epoch", "hash", "bytes"]),
    ("key_packages", &["id", "room_id", "device", "ref", "last_resort", "uploaded_at", "expires_at", "bytes"]),
    ("spent_key_packages", &["ref", "at"]),
    ("welcomes", &["id", "room_id", "device", "group_id", "at", "epoch"]),
    ("welcome_bytes", &["group_id", "epoch"]),
    ("welcome_ids", &["one", "last"]),
    ("sealed_keys", &["room_id", "group_id", "epoch", "writer", "room_epoch", "recovery_hpke_key", "has_mac", "change"]),
    ("recovery_links", &["room_id", "room_epoch", "recovery_hpke_key", "change"]),
    ("recovery_keys_held", &["room_id", "key"]),
    ("recoveries", &["recovery_id", "room_id", "recovery_key", "opened_at", "expires_at", "finished_at", "finish_hash", "finish_answer", "parts", "bytes", "memo_bytes"]),
    ("recovery_parts", &["recovery_id", "n", "group_id", "body", "body_hash", "commit_key"]),
    ("recovery_memo", &["recovery_id", "key", "value"]),
    ("envelopes", &["id", "room_id", "change", "group_id", "epoch", "sender", "seq", "prev", "hash", "recipient", "kind", "flags", "time", "received_at", "timeline", "object_id", "object_type", "object_state", "urgency", "answered_at", "object_ref", "register_id", "file_ids", "padded_size", "void_code", "cut", "header", "nonce", "body_hash", "signature"]),
    ("epoch_counts", &["group_id", "epoch", "envelopes"]),
    ("cards", OBJECT_INDEX),
    ("permission_requests", OBJECT_INDEX),
    ("artifacts", OBJECT_INDEX),
    ("notes", OBJECT_INDEX),
    ("chats", &["room_id", "timeline", "group_id", "item_count", "last_change"]),
    ("boards", &["room_id", "timeline", "group_id", "item_count", "last_change"]),
    ("registers", &["room_id", "group_id", "writer", "register_id", "head_change"]),
    ("board_frontiers", &["room_id", "board", "device", "bound", "frontier", "files", "at", "snapshot_seq", "register_id", "counts", "waiting"]),
    ("files", &["room_id", "file_id", "uploader", "group_id", "object_id", "size", "sha256", "stored_at", "referenced_at", "deleted_at"]),
    ("shares", &["share_id", "room_id", "file_id", "expires_at", "created_by", "created_at"]),
    ("invites", &["invite_id", "room_id", "inviter", "role", "offer", "offer_signature", "reveal", "reveal_signature", "revealed_request", "revealed_ref", "revealed_device", "created_at", "expires_at", "used_at", "burned_at"]),
    ("invite_requests", &["invite_id", "request_hash", "request", "signature", "key_package_ref", "device", "at"]),
    ("push_subscriptions", &["id", "room_id", "device", "kind", "environment", "topic", "level", "created_at"]),
    ("live_activities", &["room_id", "device", "environment", "topic", "tag", "started_at", "sent_working", "sent_waiting", "sent_at", "created_at"]),
];

const OBJECT_INDEX: &[&str] = &["room_id", "object_id", "group_id", "state", "urgency", "answered_at", "owner", "first_change", "head_change", "version_hash", "version_change", "settled_at", "closed_at", "pruned_at"];

/// The credentials, never shown: what a password or the code is checked against (salts, hashes, the KDF record),
/// the MACs of a recovery and of an invite, the hash of a share's secret, push endpoints and their keys, Apple's
/// device and Live Activity tokens. (Masked by being absent from `FULL`; named here so that every column of the
/// schema is classified, which the tables page checks.)
pub const SECRET: &[(&str, &[&str])] = &[
    ("accounts", &["auth_salt", "auth_hash", "kdf", "kit_salt", "kit_hash"]),
    ("group_log", &["recovery_auth"]),
    ("shares", &["secret_hash"]),
    ("invites", &["offer_mac"]),
    ("invite_requests", &["mac"]),
    ("push_subscriptions", &["endpoint", "p256dh", "auth", "apns_key"]),
    ("live_activities", &["start_token", "activity_token"]),
];

/// For a table without rowid, the column its newest rows are found by (else its key, highest first).
const NEWEST: &[(&str, &str)] = &[
    ("account_sources", "last_at"),
    ("login_accounts", "budget_start"),
    ("rooms", "founded_at"),
    ("agent_leases", "expires_at"),
    ("groups", "epoch_at"),
    ("group_log", "at"),
    ("spent_key_packages", "at"),
    ("recoveries", "opened_at"),
    ("board_frontiers", "at"),
    ("files", "stored_at"),
    ("shares", "created_at"),
    ("invites", "created_at"),
    ("invite_requests", "at"),
    ("live_activities", "created_at"),
];

fn listed(list: &[(&str, &[&str])], table: &str, column: &str) -> bool {
    list.iter().any(|(t, cols)| *t == table && cols.contains(&column))
}

/// How a column's values appear on the rows pages.
pub fn show(table: &str, column: &str) -> Show {
    if SEALED.contains(&(table, column)) {
        Show::Sealed
    } else if (table, column) == ("accounts", "email") {
        Show::Email
    } else if listed(FULL, table, column) {
        Show::Full
    } else {
        Show::Secret
    }
}

/// `table.column` of every column of the listed tables that no list names (shown masked meanwhile).
pub fn unclassified(c: &Connection) -> rusqlite::Result<Vec<String>> {
    let mut out = Vec::new();
    for (_, table, _, _) in TABLES {
        for (name, _, _) in columns(c, table)? {
            let named = SEALED.contains(&(*table, name.as_str()))
                || (*table, name.as_str()) == ("accounts", "email")
                || listed(FULL, table, &name)
                || listed(SECRET, table, &name);
            if !named {
                out.push(format!("{table}.{name}"));
            }
        }
    }
    Ok(out)
}

// ---- what a request asks ----------------------------------------------------------------------------------------

/// A rows page's question, from its query string.
#[derive(Clone, Default, PartialEq, Debug)]
pub struct Query {
    /// one column and the value it must have (empty: null)
    pub filter: Option<(String, String)>,
    /// the column sorted by (`_row`: by the key); none: newest first
    pub sort: Option<String>,
    pub asc: bool,
    /// the key the page starts after, or ends before
    pub after: Option<String>,
    pub before: Option<String>,
}

/// The sort that names the key itself.
const BY_KEY: &str = "_row";

impl Query {
    pub fn parse(query: &str) -> Query {
        let mut q = Query::default();
        let (mut column, mut value) = (None, None);
        for pair in query.split('&').filter(|p| !p.is_empty()) {
            let (name, raw) = pair.split_once('=').unwrap_or((pair, ""));
            let Some(text) = crate::admin::unescape(raw.as_bytes()).and_then(|b| String::from_utf8(b).ok()) else {
                continue;
            };
            match name {
                "f" if !text.is_empty() => column = Some(text),
                "v" => value = Some(text),
                "s" if !text.is_empty() => q.sort = Some(text),
                "d" => q.asc = text == "asc",
                "a" if !text.is_empty() => q.after = Some(text),
                "b" if !text.is_empty() => q.before = Some(text),
                _ => {}
            }
        }
        q.filter = column.map(|c| (c, value.unwrap_or_default()));
        q
    }

    /// This question's link, sorted as given and starting where given.
    fn href(&self, table: &str, sort: Option<&str>, asc: bool, after: Option<&str>, before: Option<&str>) -> String {
        let mut parts = Vec::new();
        if let Some((column, value)) = &self.filter {
            parts.push(format!("f={}", enc(column)));
            parts.push(format!("v={}", enc(value)));
        }
        if let Some(s) = sort {
            parts.push(format!("s={}", enc(s)));
        }
        if asc {
            parts.push("d=asc".into());
        }
        if let Some(a) = after {
            parts.push(format!("a={}", enc(a)));
        }
        if let Some(b) = before {
            parts.push(format!("b={}", enc(b)));
        }
        if parts.is_empty() {
            format!("/tables/{table}")
        } else {
            format!("/tables/{table}?{}", parts.join("&amp;"))
        }
    }
}

/// Percent-encoding for a query value.
fn enc(text: &str) -> String {
    text.bytes()
        .map(|b| match b {
            b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'-' | b'_' | b'.' => (b as char).to_string(),
            b => format!("%{b:02X}"),
        })
        .collect()
}

/// A row's key as its page names it.
pub fn key_of(values: &[Value]) -> String {
    values
        .iter()
        .map(|v| match v {
            Value::Integer(n) => n.to_string(),
            Value::Text(t) => format!("t{}", hex(t.as_bytes())),
            Value::Blob(b) => format!("x{}", hex(b)),
            // (a key is never null nor a fraction)
            Value::Real(x) => x.to_string(),
            Value::Null => "n".into(),
        })
        .collect::<Vec<_>>()
        .join(".")
}

/// A key named in a link, if it has the form and the number of parts this table's key has.
fn parse_key(token: &str, parts: usize) -> Option<Vec<Value>> {
    let values: Vec<Value> = token
        .split('.')
        .map(|part| {
            if let Some(h) = part.strip_prefix('x') {
                unhex(h).map(Value::Blob)
            } else if let Some(h) = part.strip_prefix('t') {
                unhex(h).and_then(|b| String::from_utf8(b).ok()).map(Value::Text)
            } else {
                part.parse::<i64>().ok().map(Value::Integer)
            }
        })
        .collect::<Option<_>>()?;
    (values.len() == parts).then_some(values)
}

// ---- the schema of one table --------------------------------------------------------------------------------------

struct Col {
    name: String,
    kind: String,
    show: Show,
}

struct Shape {
    cols: Vec<Col>,
    /// the key's columns, quoted: `rowid`, or the primary key in its order
    key: Vec<String>,
    rowid: bool,
}

fn quote(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}

fn shape(c: &Connection, table: &'static str) -> rusqlite::Result<Shape> {
    let info = columns(c, table)?;
    let sql: String = c.query_row("SELECT sql FROM sqlite_master WHERE type = 'table' AND name = ?1", [table], |r| r.get(0))?;
    let rowid = !sql.to_ascii_uppercase().contains("WITHOUT ROWID");
    let key = if rowid {
        vec!["rowid".to_string()]
    } else {
        let mut pk: Vec<(i64, &str)> = info.iter().filter(|c| c.2 > 0).map(|c| (c.2, c.0.as_str())).collect();
        pk.sort();
        pk.into_iter().map(|(_, n)| quote(n)).collect()
    };
    let cols = info
        .into_iter()
        .map(|(name, kind, _)| Col { show: show(table, &name), name, kind })
        .collect();
    Ok(Shape { cols, key, rowid })
}

impl Shape {
    fn col(&self, name: &str) -> Option<&Col> {
        self.cols.iter().find(|c| c.name == name)
    }

    /// `(a, b)` or `a`, and as many placeholders
    fn key_tuple(&self) -> (String, String) {
        let marks = vec!["?"; self.key.len()].join(", ");
        if self.key.len() == 1 {
            (self.key[0].clone(), marks)
        } else {
            (format!("({})", self.key.join(", ")), format!("({marks})"))
        }
    }
}

/// A filter's value as the column's type has it.
fn typed(kind: &str, text: &str) -> Result<Value, &'static str> {
    let text = text.trim();
    match kind.to_ascii_uppercase().as_str() {
        "INTEGER" | "INT" => text.parse().map(Value::Integer).map_err(|_| "a whole number"),
        "REAL" => text.parse().map(Value::Real).map_err(|_| "a number"),
        "BLOB" => unhex(text.strip_prefix("0x").unwrap_or(text)).map(Value::Blob).ok_or("bytes in hex"),
        _ => Ok(Value::Text(text.to_string())),
    }
}

// ---- one value --------------------------------------------------------------------------------------------------

/// One value as its column shows it, in the list (`whole` false) or on the row's own page.
fn cell(col: &Col, value: &Value, whole: bool) -> String {
    let length = |v: &Value| match v {
        Value::Blob(b) => b.len(),
        Value::Text(t) => t.len(),
        Value::Integer(_) | Value::Real(_) => 8,
        Value::Null => 0,
    };
    if *value == Value::Null {
        return "<span class=\"faint\">null</span>".into();
    }
    match col.show {
        Show::Secret => format!("<span class=\"secret\">•••• (secret, {} bytes)</span>", length(value)),
        Show::Sealed => match value {
            Value::Blob(b) => format!(
                "<span class=\"cm cm-e2e\">{LOCK}<span>{} bytes{}</span></span>",
                b.len(),
                if b.is_empty() { String::new() } else { format!(" · <code>{}</code>{}", hex(&b[..b.len().min(BYTES_SHOWN)]), if b.len() > BYTES_SHOWN { "…" } else { "" }) }
            ),
            other => format!("<span class=\"cm cm-e2e\">{LOCK}<span>{} bytes</span></span>", length(other)),
        },
        Show::Email => match value {
            Value::Text(t) => blurred_email(t),
            other => format!("<span class=\"secret\">•••• ({} bytes)</span>", length(other)),
        },
        Show::Full => match value {
            Value::Null => unreachable!(),
            Value::Integer(n) if is_time(&col.name) && (100_000_000_000..100_000_000_000_000).contains(n) => {
                format!("<span class=\"t\">{} UTC</span> <span class=\"faint\">{n}</span>", when_exactly(*n))
            }
            Value::Integer(n) => n.to_string(),
            Value::Real(x) => x.to_string(),
            Value::Text(t) if whole => format!("<span class=\"text\">{}</span>", esc(t)),
            Value::Text(t) => {
                let cut: String = t.chars().take(TEXT_SHOWN).collect();
                format!("{}{}", esc(&cut), if cut.len() < t.len() { "<span class=\"faint\">…</span>" } else { "" })
            }
            Value::Blob(b) if b.is_empty() => "<span class=\"faint\">0 bytes</span>".into(),
            Value::Blob(b) if whole => format!(
                "<span class=\"faint\">{} bytes{}</span><code class=\"hex\">{}</code>",
                b.len(),
                if b.len() > BYTES_IN_FULL { format!(", the first {BYTES_IN_FULL} in hex") } else { String::new() },
                hex(&b[..b.len().min(BYTES_IN_FULL)])
            ),
            Value::Blob(b) => format!(
                "<span class=\"faint\">{} bytes</span> <code>{}</code>{}",
                b.len(),
                hex(&b[..b.len().min(BYTES_SHOWN)]),
                if b.len() > BYTES_SHOWN { "<span class=\"faint\">…</span>" } else { "" }
            ),
        },
    }
}

/// The head of a table's pages: its name, group and number of rows; its columns (a card, folded); what the hub
/// reads in it and what lies sealed; and in one line which columns are not shown in full.
fn head(table: &str, n: i64, extra: &str, shape: &Shape) -> String {
    let (group, reads, sealed) = TABLES
        .iter()
        .find(|t| t.1 == table)
        .map(|t| (t.0, t.2, t.3))
        .unwrap_or_default();
    let mut out = format!(
        "<div class=\"head\"><h1><code class=\"big\">{table}</code></h1><span class=\"muted\">{group} · {n} row{}{extra}</span></div>",
        if n == 1 { "" } else { "s" },
    );
    // the columns: name, type, a lock for sealed, dots for a credential; the key's in bold
    out.push_str(&format!(
        "<details class=\"card cols-card\"><summary><h3>Columns <span class=\"count\">{}</span></h3></summary><ul class=\"colchips\">",
        shape.cols.len()
    ));
    for col in &shape.cols {
        let name = if shape.key.contains(&quote(&col.name)) { format!("<b>{}</b>", esc(&col.name)) } else { esc(&col.name) };
        out.push_str(&format!(
            "<li>{}<code>{name}</code><span class=\"faint\">{}</span></li>",
            match col.show {
                Show::Sealed => format!("<span class=\"cm cm-e2e\" title=\"sealed\">{LOCK}</span>"),
                Show::Secret => "<span class=\"secret\" title=\"credential: not shown\">••</span>".into(),
                _ => String::new(),
            },
            esc(&col.kind.to_lowercase())
        ));
    }
    out.push_str("</ul></details>");
    out.push_str(&format!(
        "<p class=\"legend\"><span class=\"cm cm-plain\">{EYE}<span>The hub reads: {}</span></span><span class=\"cm cm-e2e\">{LOCK}<span>Sealed: {}</span></span></p>",
        esc(reads),
        if sealed.is_empty() { "nothing".into() } else { esc(sealed) }
    ));
    // which columns are not shown in full, in one line
    let named = |show: Show| shape.cols.iter().filter(|c| c.show == show).map(|c| esc(&c.name)).collect::<Vec<_>>().join(", ");
    let mut masked = Vec::new();
    for (show, why) in [
        (Show::Secret, "credentials, shown by their length only"),
        (Show::Sealed, "sealed, shown by their length and first bytes"),
        (Show::Email, "blurred until double-clicked"),
    ] {
        let cols = named(show);
        if !cols.is_empty() {
            masked.push(format!("<code>{cols}</code> ({why})"));
        }
    }
    out.push_str(&format!(
        "<p class=\"masked muted\">{}</p>",
        if masked.is_empty() { "Every column is shown in full.".to_string() } else { format!("Not in full: {}.", masked.join(" · ")) }
    ));
    out
}

// ---- the pages --------------------------------------------------------------------------------------------------

/// The ORDER BY of the rows, and its reverse for a page that ends before a key.
fn order_by(sort: Option<&str>, key: &[String], asc: bool) -> String {
    let dir = if asc { "ASC" } else { "DESC" };
    sort.into_iter()
        .map(String::from)
        .chain(key.iter().cloned())
        .map(|c| format!("{c} {dir}"))
        .collect::<Vec<_>>()
        .join(", ")
}

/// The rows beyond one row (`greater`: after it in ascending order, else before it), in an order by `sort` and
/// then the key. SQLite sorts null first; the key is never null.
fn beyond(shape: &Shape, sort: Option<&str>, greater: bool, at: &(Vec<Value>, Value), params: &mut Vec<Value>) -> String {
    let (k, marks) = shape.key_tuple();
    let op = if greater { ">" } else { "<" };
    match sort {
        None => {
            params.extend(at.0.iter().cloned());
            format!("{k} {op} {marks}")
        }
        Some(s) if at.1 == Value::Null => {
            params.extend(at.0.iter().cloned());
            if greater {
                format!("({s} IS NOT NULL OR {k} > {marks})")
            } else {
                format!("({s} IS NULL AND {k} < {marks})")
            }
        }
        Some(s) => {
            params.extend([at.1.clone(), at.1.clone()]);
            params.extend(at.0.iter().cloned());
            if greater {
                format!("({s} > ? OR ({s} = ? AND {k} > {marks}))")
            } else {
                format!("({s} < ? OR {s} IS NULL OR ({s} = ? AND {k} < {marks}))")
            }
        }
    }
}

/// One table's rows, as `q` asks.
pub fn rows(app: &App, table: &'static str, q: &Query) -> Result<String, Refused> {
    app.db.read(|c| {
        let (side, _) = side(c, Some(table))?;
        let shape = shape(c, table)?;
        let mut notes = Vec::new();

        // the filter: a column shown in full (or the e-mail), its value in the column's type
        let mut filter_sql = String::from("1");
        let mut filter_params = Vec::new();
        let mut filter = None;
        if let Some((name, value)) = &q.filter {
            match shape.col(name) {
                None => notes.push(format!("No column {} here.", esc(name))),
                Some(col) if matches!(col.show, Show::Secret | Show::Sealed) => {
                    notes.push(format!("{} cannot be filtered by: its values are not shown.", esc(name)))
                }
                Some(col) if value.is_empty() => {
                    filter_sql = format!("{} IS NULL", quote(&col.name));
                    filter = Some((col.name.clone(), value.clone()));
                }
                Some(col) => match typed(&col.kind, value) {
                    Ok(v) => {
                        filter_sql = format!("{} = ?", quote(&col.name));
                        filter_params.push(v);
                        filter = Some((col.name.clone(), value.clone()));
                    }
                    Err(want) => notes.push(format!("{} takes {want}.", esc(name))),
                },
            }
        }

        // the sort: a column shown in full, or the key; by default the newest first
        let mut sort_name = q.sort.clone();
        if let Some(name) = &q.sort {
            if name != BY_KEY && !matches!(shape.col(name).map(|c| c.show), Some(Show::Full | Show::Email)) {
                notes.push(format!("Not sorted by {}: no such column, or its values are not shown.", esc(name)));
                sort_name = None;
            }
        }
        let q = Query { filter, sort: sort_name.clone(), ..q.clone() };
        let default_sort = if shape.rowid { None } else { NEWEST.iter().find(|n| n.0 == table).map(|n| n.1) };
        let sort_col: Option<String> = match sort_name.as_deref() {
            Some(BY_KEY) => None,
            Some(name) => Some(name.to_string()),
            None => default_sort.map(String::from),
        };
        let sort_sql = sort_col.as_deref().map(quote);
        let asc = q.asc;

        let n: i64 = c.query_row(&format!("SELECT count(*) FROM {table} WHERE {filter_sql}"), rusqlite::params_from_iter(&filter_params), |r| r.get(0))?;

        // where the page starts: a key, and the sort value of its row
        let (k, marks) = shape.key_tuple();
        let at_key = |token: &str| -> rusqlite::Result<Option<(Vec<Value>, Value)>> {
            let Some(key) = parse_key(token, shape.key.len()) else { return Ok(None) };
            let sql = format!("SELECT {} FROM {table} WHERE {k} = {marks}", sort_sql.as_deref().unwrap_or("NULL"));
            let mut s = c.prepare(&sql)?;
            let mut found = s.query(rusqlite::params_from_iter(&key))?;
            Ok(match found.next()? {
                Some(r) => Some((key.clone(), r.get::<_, Value>(0)?)),
                None => None,
            })
        };
        let (from, backwards) = match (&q.after, &q.before) {
            (Some(a), _) => (at_key(a)?, false),
            (None, Some(b)) => (at_key(b)?, true),
            (None, None) => (None, false),
        };
        if (q.after.is_some() || q.before.is_some()) && from.is_none() {
            notes.push("That row is not there any more: from the start.".into());
        }

        // the page: its rows as (key, sort value, values)
        let fetch = |cond: &str, params: Vec<Value>, asc: bool, limit: usize| -> rusqlite::Result<Vec<(Vec<Value>, Value, Vec<Value>)>> {
            let sql = format!(
                "SELECT {}, {}, * FROM {table} WHERE {filter_sql} AND {cond} ORDER BY {} LIMIT {limit}",
                shape.key.join(", "),
                sort_sql.as_deref().unwrap_or("NULL"),
                order_by(sort_sql.as_deref(), &shape.key, asc)
            );
            let mut s = c.prepare(&sql)?;
            let all: Vec<Value> = filter_params.iter().cloned().chain(params).collect();
            let mut found = s.query(rusqlite::params_from_iter(&all))?;
            let (kn, cn) = (shape.key.len(), shape.cols.len());
            let mut out = Vec::new();
            while let Some(r) = found.next()? {
                let key = (0..kn).map(|i| r.get::<_, Value>(i)).collect::<rusqlite::Result<Vec<_>>>()?;
                let sv = r.get::<_, Value>(kn)?;
                let values = (0..cn).map(|i| r.get::<_, Value>(kn + 1 + i)).collect::<rusqlite::Result<Vec<_>>>()?;
                out.push((key, sv, values));
            }
            Ok(out)
        };
        let mut page = match &from {
            None => fetch("1", Vec::new(), asc, PAGE)?,
            Some(at) => {
                let mut params = Vec::new();
                // after a row in the order shown, or before it (read in the reverse order, then turned)
                let cond = beyond(&shape, sort_sql.as_deref(), asc != backwards, at, &mut params);
                fetch(&cond, params, asc != backwards, PAGE)?
            }
        };
        if from.is_some() && backwards {
            page.reverse();
        }
        let more = |row: &(Vec<Value>, Value, Vec<Value>), greater: bool| -> rusqlite::Result<bool> {
            let mut params = Vec::new();
            let cond = beyond(&shape, sort_sql.as_deref(), greater, &(row.0.clone(), row.1.clone()), &mut params);
            Ok(!fetch(&cond, params, greater, 1)?.is_empty())
        };
        let (earlier, later) = match (page.first(), page.last()) {
            (Some(first), Some(last)) => (more(first, !asc)?, more(last, asc)?),
            _ => (false, false),
        };

        // ---- the page
        let mut main = head(table, n, if q.filter.is_some() { " where the filter holds" } else { "" }, &shape);
        for note in &notes {
            main.push_str(&format!("<p class=\"err\">{note}</p>"));
        }
        main.push_str("<section class=\"card\">");
        // the filter: one column, one value (a GET form: nothing changes)
        main.push_str(&format!("<form class=\"filter\" method=\"get\" action=\"/tables/{table}\"><label>Column <select name=\"f\">"));
        for col in shape.cols.iter().filter(|c| matches!(c.show, Show::Full | Show::Email)) {
            let on = q.filter.as_ref().is_some_and(|f| f.0 == col.name);
            main.push_str(&format!("<option{}>{}</option>", if on { " selected" } else { "" }, esc(&col.name)));
        }
        main.push_str(&format!(
            "</select></label><label>is <input type=\"text\" name=\"v\" value=\"{}\" placeholder=\"value (bytes in hex; empty: null)\" spellcheck=\"false\"></label>{}{}<button>Filter</button>{}</form>",
            esc(q.filter.as_ref().map_or("", |f| f.1.as_str())),
            q.sort.as_deref().map_or(String::new(), |s| format!("<input type=\"hidden\" name=\"s\" value=\"{}\">", esc(s))),
            if asc { "<input type=\"hidden\" name=\"d\" value=\"asc\">" } else { "" },
            if q.filter.is_some() { format!("<a href=\"{}\">Clear</a>", Query { filter: None, ..Query::default() }.href(table, q.sort.as_deref(), asc, None, None)) } else { String::new() }
        ));

        if page.is_empty() {
            main.push_str(&format!("<p class=\"muted\">{}</p>", if n == 0 { "No rows." } else { "No rows here." }));
        } else {
            // the head: each column sorts (again: the other way round); secret ones do not
            let arrow = |on: bool| if !on { "" } else if asc { " ▲" } else { " ▼" };
            let key_sorted = sort_col.is_none();
            main.push_str(&format!(
                "<div class=\"scroll\"><table class=\"grid rows\"><thead><tr><th><a href=\"{}\" title=\"by the key\">{}{}</a></th>",
                q.href(table, Some(BY_KEY), key_sorted && !asc, None, None),
                if shape.rowid { "rowid" } else { "key" },
                arrow(key_sorted)
            ));
            for col in &shape.cols {
                let on = sort_col.as_deref() == Some(col.name.as_str());
                if matches!(col.show, Show::Full | Show::Email) {
                    main.push_str(&format!(
                        "<th><a href=\"{}\">{}{}</a></th>",
                        q.href(table, Some(&col.name), on && !asc, None, None),
                        esc(&col.name),
                        arrow(on)
                    ));
                } else {
                    main.push_str(&format!("<th class=\"muted\">{}</th>", esc(&col.name)));
                }
            }
            main.push_str("</tr></thead><tbody>");
            for (key, _, values) in &page {
                let token = key_of(key);
                main.push_str(&format!(
                    "<tr><td><a href=\"/tables/{table}/{}\">{}</a></td>",
                    enc(&token),
                    if shape.rowid { esc(&token) } else { "open".into() }
                ));
                for (col, value) in shape.cols.iter().zip(values) {
                    main.push_str(&format!("<td>{}</td>", cell(col, value, false)));
                }
                main.push_str("</tr>");
            }
            main.push_str("</tbody></table></div>");
        }
        // the pages before and after
        let sort_param = q.sort.as_deref();
        main.push_str("<nav class=\"pager\">");
        if from.is_some() {
            main.push_str(&format!("<a href=\"{}\">First</a>", q.href(table, sort_param, asc, None, None)));
        }
        match page.first() {
            Some(first) if earlier => main.push_str(&format!("<a href=\"{}\">← Previous</a>", q.href(table, sort_param, asc, None, Some(&key_of(&first.0))))),
            _ => main.push_str("<span class=\"faint\">← Previous</span>"),
        }
        match page.last() {
            Some(last) if later => main.push_str(&format!("<a href=\"{}\">Next →</a>", q.href(table, sort_param, asc, Some(&key_of(&last.0)), None))),
            _ => main.push_str("<span class=\"faint\">Next →</span>"),
        }
        main.push_str(&format!(
            "<span class=\"faint\">{} row{} on this page · {PAGE} at most · times in UTC</span></nav></section>",
            page.len(),
            if page.len() == 1 { "" } else { "s" }
        ));

        Ok::<_, Refused>(framed(Some(table), &side, &main))
    })
}

/// One row with every value as its column shows it, or `None` if there is no such row.
pub fn row(app: &App, table: &'static str, token: &str) -> Result<Option<String>, Refused> {
    app.db.read(|c| {
        let shape = shape(c, table)?;
        let Some(key) = parse_key(token, shape.key.len()) else { return Ok(None) };
        let (k, marks) = shape.key_tuple();
        let mut s = c.prepare(&format!("SELECT * FROM {table} WHERE {k} = {marks}"))?;
        let mut found = s.query(rusqlite::params_from_iter(&key))?;
        let Some(r) = found.next()? else { return Ok(None) };
        let values = (0..shape.cols.len()).map(|i| r.get::<_, Value>(i)).collect::<rusqlite::Result<Vec<_>>>()?;
        let (side, counts) = side(c, Some(table))?;
        let n = TABLES.iter().position(|t| t.1 == table).map_or(0, |i| counts[i]);

        let mut main = head(table, n, "", &shape);
        main.push_str(&format!(
            "<section class=\"card\"><h3><span>Row <code>{}</code></span><a class=\"count\" href=\"/tables/{table}\">← all rows</a></h3><dl class=\"kv row\">",
            esc(&if shape.rowid { token.to_string() } else { format!("{}…", &token[..token.len().min(24)]) })
        ));
        for (col, value) in shape.cols.iter().zip(&values) {
            // a value shown in full is a filter for its table (a link, no form)
            let filter = match (col.show, value) {
                (Show::Full, Value::Integer(n)) => Some(n.to_string()),
                (Show::Full, Value::Blob(b)) if !b.is_empty() && b.len() <= 64 => Some(hex(b)),
                (Show::Full, Value::Text(t)) if t.len() <= 200 => Some(t.clone()),
                _ => None,
            };
            main.push_str(&format!(
                "<dt><code>{}</code><span class=\"faint\">{}</span></dt><dd>{}{}</dd>",
                esc(&col.name),
                esc(&col.kind.to_lowercase()),
                cell(col, value, true),
                filter.map_or(String::new(), |v| format!(
                    " <a class=\"same\" href=\"/tables/{table}?f={}&amp;v={}\">rows with this value</a>",
                    enc(&col.name),
                    enc(&v)
                ))
            ));
        }
        main.push_str("</dl></section>");
        Ok::<_, Refused>(Some(framed(Some(table), &side, &main)))
    })
}

/// The table a path names (`/tables/<name>` or `/tables/<name>/<key>`), and the key if one is named.
pub fn path(rest: &str) -> Option<(&'static str, Option<&str>)> {
    let (name, key) = match rest.split_once('/') {
        Some((name, key)) => (name, Some(key)),
        None => (rest, None),
    };
    let table = table_named(name)?;
    match key {
        Some(k) if k.is_empty() || !k.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'-') => None,
        _ => Some((table, key)),
    }
}
