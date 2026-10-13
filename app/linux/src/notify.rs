//! Desktop notifications while the app runs (the Desktop Notifications Specification over the session D-Bus,
//! `org.freedesktop.Notifications`): a new question, a new urgent card and a permission request each say so once;
//! a click on one opens its card in the window. What the Desk held before the live stream first opened (the cache and
//! the catch-up after a start or a sign-in) is not announced, nor anything while the window has the focus. `TROMMI_LINUX_NOTIFY=0` turns them off.

use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

/// Which rows of a Desk are new and worth a notification: kept per run.
#[derive(Default)]
pub struct Watch {
    seen: HashSet<String>,
    primed: bool,
}

/// A notification to show: the card it opens, its summary and body, and whether it is urgent.
#[derive(Debug, Clone, PartialEq)]
pub struct Note {
    pub card: String,
    pub summary: String,
    pub body: String,
    pub urgent: bool,
}

impl Watch {
    /// The room is caught up and live: from now on new rows are announced.
    pub fn go_live(&mut self) {
        self.primed = true;
    }

    /// The Desk's rows (`view.rs desk_json`) that came since the last look and ask for the person: a decision card,
    /// an urgent card, a permission request. Before `go_live` it only learns what is there.
    pub fn fresh(&mut self, desk_json: &str) -> Vec<Note> {
        let rows: Vec<Value> = serde_json::from_str(desk_json).unwrap_or_default();
        let primed = self.primed;
        let mut out = Vec::new();
        for r in &rows {
            let Some(id) = r["id"].as_str() else { continue };
            if !self.seen.insert(id.to_string()) || !primed {
                continue;
            }
            let urgent = matches!(r["urgency"].as_str(), Some("high" | "critical"));
            let request = r["kind"] == "request";
            let question = r["kind"] == "card" && r["card_type"] != "info" && r["answered"] != true;
            if !(request || question || urgent) {
                continue;
            }
            let session = r["session"].as_str().filter(|s| !s.is_empty()).unwrap_or("Trommi");
            let title = r["title"].as_str().unwrap_or("").trim();
            let (summary, body) = if request {
                (format!("{session} asks to use {title}"), r["body"].as_str().unwrap_or("").to_string())
            } else {
                let teaser = r["teaser"].as_str().filter(|t| !t.is_empty()).or(r["body"].as_str()).unwrap_or("");
                (format!("{session}: {title}"), first_line(teaser, 160))
            };
            out.push(Note { card: id.to_string(), summary, body, urgent });
        }
        out
    }
}

/// The first line of a text, at most `max` characters.
fn first_line(text: &str, max: usize) -> String {
    let line = text.lines().find(|l| !l.trim().is_empty()).unwrap_or("").trim();
    if line.chars().count() <= max {
        line.to_string()
    } else {
        format!("{} …", line.chars().take(max).collect::<String>().trim_end())
    }
}

#[zbus::proxy(
    interface = "org.freedesktop.Notifications",
    default_service = "org.freedesktop.Notifications",
    default_path = "/org/freedesktop/Notifications"
)]
trait Notifications {
    #[allow(clippy::too_many_arguments)]
    fn notify(
        &self,
        app_name: &str,
        replaces_id: u32,
        app_icon: &str,
        summary: &str,
        body: &str,
        actions: &[&str],
        hints: HashMap<&str, zbus::zvariant::Value<'_>>,
        expire_timeout: i32,
    ) -> zbus::Result<u32>;

    #[zbus(signal)]
    fn action_invoked(&self, id: u32, action_key: String) -> zbus::Result<()>;

    #[zbus(signal)]
    fn notification_closed(&self, id: u32, reason: u32) -> zbus::Result<()>;
}

/// The session bus's notification server, and which card each shown notification opens.
pub struct Notifier {
    proxy: NotificationsProxy<'static>,
    shown: Arc<Mutex<HashMap<u32, String>>>,
}

impl Notifier {
    /// Connects to the session bus and follows the clicks: `open` gets the card a clicked notification names.
    pub async fn connect(open: Arc<dyn Fn(String) + Send + Sync>) -> zbus::Result<Self> {
        use futures_util::StreamExt;
        let conn = zbus::Connection::session().await?;
        let proxy = NotificationsProxy::new(&conn).await?;
        let shown: Arc<Mutex<HashMap<u32, String>>> = Arc::default();
        let mut clicks = proxy.receive_action_invoked().await?;
        let mut closed = proxy.receive_notification_closed().await?;
        let map = shown.clone();
        tokio::spawn(async move {
            while let Some(s) = clicks.next().await {
                let Ok(a) = s.args() else { continue };
                let card = map.lock().unwrap_or_else(|e| e.into_inner()).remove(&a.id);
                if let Some(card) = card {
                    open(card);
                }
            }
        });
        let map = shown.clone();
        tokio::spawn(async move {
            while let Some(s) = closed.next().await {
                if let Ok(a) = s.args() {
                    map.lock().unwrap_or_else(|e| e.into_inner()).remove(&a.id);
                }
            }
        });
        Ok(Self { proxy, shown })
    }

    /// Shows one notification; a click (its default action) opens its card.
    pub async fn show(&self, n: &Note) -> zbus::Result<()> {
        let mut hints: HashMap<&str, zbus::zvariant::Value<'_>> = HashMap::new();
        hints.insert("urgency", zbus::zvariant::Value::U8(if n.urgent { 2 } else { 1 }));
        hints.insert("desktop-entry", zbus::zvariant::Value::from("trommi"));
        hints.insert("category", zbus::zvariant::Value::from("im.received"));
        let id = self
            .proxy
            .notify("Trommi", 0, "trommi", &n.summary, &n.body, &["default", "Open"], hints, -1)
            .await?;
        self.shown.lock().unwrap_or_else(|e| e.into_inner()).insert(id, n.card.clone());
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn desk(rows: Value) -> String {
        rows.to_string()
    }

    #[test]
    fn only_what_comes_later_and_asks() {
        let mut w = Watch::default();
        let old = json!({ "kind": "card", "id": "a", "card_type": "decision", "title": "Old", "session": "Web" });
        assert!(w.fresh(&desk(json!([old]))).is_empty(), "the catch-up only learns");
        let early = json!({ "kind": "card", "id": "x", "card_type": "decision", "title": "While away", "session": "Web" });
        assert!(w.fresh(&desk(json!([old, early]))).is_empty(), "the catch-up only learns");
        w.go_live();
        let info = json!({ "kind": "card", "id": "b", "card_type": "info", "title": "Done", "urgency": "normal", "session": "Web" });
        let ask = json!({ "kind": "card", "id": "c", "card_type": "decision", "title": "Which hero?", "teaser": "Two drafts\nmore", "session": "Web" });
        let hot = json!({ "kind": "card", "id": "d", "card_type": "info", "title": "Prod is down", "urgency": "critical", "session": "Infra" });
        let req = json!({ "kind": "request", "id": "e", "title": "Bash", "body": "rm -rf build", "urgency": "high", "session": "Infra" });
        let got = w.fresh(&desk(json!([old, info, ask, hot, req])));
        assert_eq!(got.iter().map(|n| n.card.as_str()).collect::<Vec<_>>(), ["c", "d", "e"]);
        assert_eq!(got[0].summary, "Web: Which hero?");
        assert_eq!(got[0].body, "Two drafts");
        assert!(!got[0].urgent && got[1].urgent && got[2].urgent);
        assert_eq!(got[2].summary, "Infra asks to use Bash");
        assert!(w.fresh(&desk(json!([ask]))).is_empty(), "each once");
    }
}
