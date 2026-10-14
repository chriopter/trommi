//! Desktop notifications (the Desktop Notifications Specification over the session D-Bus,
//! `org.freedesktop.Notifications`): what the app core says to notify (`Effect::Notify`), shown as it says; a click
//! opens what it names in the window. `TROMMI_LINUX_NOTIFY=0` turns them off.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// A notification to show: the card it opens, its summary and body, and whether it is urgent.
#[derive(Debug, Clone, PartialEq)]
pub struct Note {
    pub card: String,
    pub summary: String,
    pub body: String,
    pub urgent: bool,
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
