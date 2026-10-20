//! In-process event bus shared by both deployment shapes.
//!
//! Every status event (recording, danmaku, recovery, auto-record, lifecycle)
//! is published once to this bus. The Tauri desktop binary forwards bus events
//! to the webview via `app.emit`; the headless server binary forwards them to
//! browsers over Server-Sent Events. Core logic stays transport-agnostic.

use serde::Serialize;
use std::sync::Arc;
use tokio::sync::broadcast;

pub type EventPayload = serde_json::Value;

#[derive(Clone)]
pub struct EventBus {
    tx: broadcast::Sender<(String, EventPayload)>,
}

impl Default for EventBus {
    fn default() -> Self {
        Self::new()
    }
}

impl EventBus {
    pub fn new() -> Self {
        let (tx, _rx) = broadcast::channel(256);
        Self { tx }
    }

    pub fn publish(&self, event: &str, payload: impl Serialize) {
        let Ok(payload) = serde_json::to_value(payload) else {
            return;
        };
        // Dropping slow subscribers is fine: status snapshots are re-fetched
        // by the UI through the REST API.
        let _ = self.tx.send((event.to_string(), payload));
    }

    pub fn subscribe(&self) -> broadcast::Receiver<(String, EventPayload)> {
        self.tx.subscribe()
    }
}

/// Transport-agnostic host facade used by all core logic. It owns the shared
/// state and the event bus only — no UI toolkit types — so the headless server
/// binary never links GTK/WebKit through Tauri.
#[derive(Clone)]
pub struct Host {
    pub state: Arc<crate::AppState>,
    pub bus: EventBus,
}

impl Host {
    pub fn new(state: Arc<crate::AppState>) -> Self {
        let bus = state.bus.clone();
        Self { state, bus }
    }

    /// Emit an event through the bus. The desktop binary forwards bus events to
    /// the webview; the server forwards them to browsers over SSE.
    pub fn emit(&self, event: &str, payload: impl Serialize) {
        self.bus.publish(event, payload);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn bus_delivers_events_to_subscribers() {
        let bus = EventBus::new();
        let mut rx = bus.subscribe();
        bus.publish("test", serde_json::json!({"a": 1}));
        let (event, payload) = rx.recv().await.unwrap();
        assert_eq!(event, "test");
        assert_eq!(payload["a"], 1);
    }

    #[test]
    fn bus_drops_slow_subscribers_without_panicking() {
        let bus = EventBus::new();
        let rx = bus.subscribe();
        drop(rx);
        for i in 0..300 {
            bus.publish("flood", serde_json::json!({ "i": i }));
        }
    }
}
