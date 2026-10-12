use std::collections::HashMap;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use futures_util::TryStreamExt;
use serde::Serialize;
use tokio::sync::broadcast;
use zbus::zvariant::OwnedValue;
use zbus::{Connection, Message, MessageStream};

/// A desktop notification as pushed to clients.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Notification {
    pub summary: String,
    pub body: String,
    pub icon: String,
    pub appname: String,
}

/// Some desktops (e.g. GNOME) proxy each `Notify` call through a second bus
/// name, so the monitor sees the same notification twice in quick succession.
const DUPLICATE_WINDOW: Duration = Duration::from_millis(250);

type NotifyArgs = (
    String,                      // app_name
    u32,                         // replaces_id
    String,                      // app_icon
    String,                      // summary
    String,                      // body
    Vec<String>,                 // actions
    HashMap<String, OwnedValue>, // hints
    i32,                         // expire_timeout
);

impl Notification {
    fn from_message(msg: &Message) -> Result<Self> {
        let (appname, _, mut icon, summary, body, _, hints, _): NotifyArgs =
            msg.body().deserialize()?;
        if icon.is_empty() {
            let hint = hints.get("image-path").or_else(|| hints.get("image_path"));
            if let Some(path) = hint.and_then(|v| <&str>::try_from(v).ok()) {
                icon = path.to_owned();
            }
        }
        Ok(Self { summary, body, icon, appname })
    }
}

/// Eavesdrops on the session bus and broadcasts every `Notify` call.
pub async fn watch(tx: broadcast::Sender<Notification>) -> Result<()> {
    let connection = Connection::session()
        .await
        .context("connecting to the session bus")?;

    let rule = "type='method_call',interface='org.freedesktop.Notifications',member='Notify'";
    connection
        .call_method(
            Some("org.freedesktop.DBus"),
            "/org/freedesktop/DBus",
            Some("org.freedesktop.DBus.Monitoring"),
            "BecomeMonitor",
            &(&[rule][..], 0u32),
        )
        .await
        .context("becoming a bus monitor")?;

    let mut stream = MessageStream::from(connection);
    let mut last: Option<(Notification, Instant)> = None;
    while let Some(msg) = stream.try_next().await? {
        match Notification::from_message(&msg) {
            Ok(n) => {
                if last.as_ref().is_some_and(|(prev, at)| *prev == n && at.elapsed() < DUPLICATE_WINDOW) {
                    continue;
                }
                last = Some((n.clone(), Instant::now()));
                tracing::debug!(?n, "notification");
                // Err just means no client is connected right now.
                let _ = tx.send(n);
            }
            Err(e) => tracing::warn!("ignoring malformed Notify call: {e:#}"),
        }
    }
    anyhow::bail!("session bus connection closed")
}
