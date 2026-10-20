//! Danmaku (弹幕) recording for Douyin live rooms.
//!
//! One session runs per recording task. It connects to the Douyin webcast
//! WebSocket, appends chat messages to a Bilibili-compatible XML file whose
//! timestamps are offsets from the recording (or segment) start time, and
//! reports status changes through a channel so the UI can show live counts.
//!
//! With segmented recording, the session is *rotated* at every segment
//! boundary: the current XML is finalized and a new one is opened for the new
//! segment, without reconnecting the WebSocket, so every video segment pairs
//! with a danmaku file whose timeline starts at the same instant.
//!
//! References:
//! - <https://github.com/saermart/DouyinLiveWebFetcher> (protocol, MIT)
//! - <https://github.com/hua0512/rust-srec> (Rust implementation, Apache-2.0)
//! - <https://github.com/LyzenX/DouyinLiveRecorder> (XML conventions, GPL-3.0)

pub mod ass;
pub mod proto;
pub mod signature;
pub mod writer;
pub mod ws;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use chrono::{DateTime, Utc};
use tokio::sync::{mpsc, watch};
use tokio::task::JoinHandle;

use writer::SharedDanmakuWriter;
use ws::DanmakuEvent;

/// Status updates sent to the application layer for UI events/logs.
#[derive(Debug, Clone)]
pub struct DanmakuStatus {
    pub task_id: i64,
    /// Messages in the current segment/session.
    pub count: u64,
    /// Messages accumulated across rotations.
    pub total_count: u64,
    /// 1-based segment index when segmented recording is enabled.
    pub segment_index: Option<u32>,
    pub state: &'static str,
    pub error: Option<String>,
}

/// Inputs required to start a danmaku session.
pub struct DanmakuConfig {
    pub task_id: i64,
    /// Internal webcast room id (`id_str` from the room enter API). The
    /// danmaku WebSocket routes by this id, not by the URL's web_rid.
    pub room_id: String,
    /// `ttwid` cookie from the HTTP session, used for WebSocket auth.
    pub ttwid: String,
    /// Optional user-provided cookie (settings), ttwid entries ignored.
    pub cookie: String,
    /// Output XML path (Bilibili-compatible format).
    pub output_path: PathBuf,
    /// Wall-clock instant the first video segment starts; message offsets are
    /// relative to it.
    pub start_time: DateTime<Utc>,
    /// 1-based segment index (1 when not segmented).
    pub segment_index: u32,
}

/// Commands the processor task accepts without dropping the WebSocket.
enum ProcessorCommand {
    /// Finalize the current XML and open a new one for the next segment.
    Rotate {
        path: PathBuf,
        start_time: DateTime<Utc>,
        segment_index: u32,
    },
}

struct Session {
    shutdown: watch::Sender<bool>,
    commands: mpsc::UnboundedSender<ProcessorCommand>,
    task: JoinHandle<()>,
    segment_index: Arc<AtomicU64>,
    path: Arc<std::sync::Mutex<PathBuf>>,
    count: Arc<AtomicU64>,
    total_count: Arc<AtomicU64>,
}

/// Owns all active danmaku sessions, keyed by recording task id.
pub struct DanmakuManager {
    sessions: Mutex<HashMap<i64, Session>>,
}

impl Default for DanmakuManager {
    fn default() -> Self {
        Self::new()
    }
}

impl DanmakuManager {
    pub fn new() -> Self {
        Self {
            sessions: Mutex::new(HashMap::new()),
        }
    }

    /// Start a danmaku session for a recording task.
    pub fn start(
        &self,
        config: DanmakuConfig,
        status_tx: mpsc::UnboundedSender<DanmakuStatus>,
    ) -> Result<(), String> {
        let mut sessions = self
            .sessions
            .lock()
            .map_err(|_| "弹幕管理器锁不可用".to_string())?;
        if sessions.contains_key(&config.task_id) {
            return Err("该录制任务的弹幕录制已经在运行".to_string());
        }

        let task_id = config.task_id;
        let count = Arc::new(AtomicU64::new(0));
        let total_count = Arc::new(AtomicU64::new(0));
        let (shutdown_tx, shutdown_rx) = watch::channel(false);
        let (command_tx, mut command_rx) = mpsc::unbounded_channel::<ProcessorCommand>();
        let shared_index = Arc::new(AtomicU64::new(u64::from(config.segment_index)));
        let shared_path = Arc::new(std::sync::Mutex::new(config.output_path.clone()));

        // The WebSocket session lives independently so rotation never
        // reconnects. Events flow through the processor below.
        let ws_session = ws::WsSession::new(&config.room_id, &config.ttwid, &config.cookie);
        let (ws_tx, mut ws_rx) = mpsc::unbounded_channel::<DanmakuEvent>();
        let ws_shutdown = shutdown_rx.clone();
        tokio::spawn(async move {
            let _ = ws::run_session(ws_session, ws_tx, ws_shutdown).await;
        });

        // Current writer slot; swapped on rotation.
        let writer: Arc<std::sync::Mutex<Option<SharedDanmakuWriter>>> =
            Arc::new(std::sync::Mutex::new(Some(SharedDanmakuWriter::new(
                &config.output_path,
                config.start_time,
            ))));

        let task_writer = writer.clone();
        let task_count = count.clone();
        let task_total = total_count.clone();
        let task_index = shared_index.clone();
        let task_path = shared_path.clone();
        let mut current_start = config.start_time;
        let mut current_index = config.segment_index;

        let processor = {
            let status_tx = status_tx.clone();
            let shutdown = shutdown_rx;
            tokio::spawn(async move {
                let mut shutdown = shutdown;
                loop {
                    tokio::select! {
                        _ = shutdown.changed() => {
                            if *shutdown.borrow() {
                                finalize(&task_writer);
                                let _ = status_tx.send(DanmakuStatus {
                                    task_id,
                                    count: task_count.load(Ordering::Relaxed),
                                    total_count: task_total.load(Ordering::Relaxed),
                                    segment_index: Some(current_index),
                                    state: "stopped",
                                    error: None,
                                });
                                return;
                            }
                        }
                        command = command_rx.recv() => {
                            match command {
                                Some(ProcessorCommand::Rotate { path, start_time, segment_index }) => {
                                    finalize(&task_writer);
                                    *task_writer.lock().expect("danmaku writer slot poisoned") =
                                        Some(SharedDanmakuWriter::new(&path, start_time));
                                    task_count.store(0, Ordering::Relaxed);
                                    current_start = start_time;
                                    current_index = segment_index;
                                    task_index.store(u64::from(segment_index), Ordering::Relaxed);
                                    if let Ok(mut slot) = task_path.lock() {
                                        *slot = path;
                                    }
                                    let _ = status_tx.send(DanmakuStatus {
                                        task_id,
                                        count: 0,
                                        total_count: task_total.load(Ordering::Relaxed),
                                        segment_index: Some(segment_index),
                                        state: "rotated",
                                        error: None,
                                    });
                                }
                                None => {
                                    // The manager dropped the command sender; finish up.
                                    finalize(&task_writer);
                                    return;
                                }
                            }
                        }
                        event = ws_rx.recv() => {
                            match event {
                                Some(DanmakuEvent::Connected) => {
                                    let _ = status_tx.send(DanmakuStatus {
                                        task_id,
                                        count: task_count.load(Ordering::Relaxed),
                                        total_count: task_total.load(Ordering::Relaxed),
                                        segment_index: Some(current_index),
                                        state: "connected", error: None,
                                    });
                                }
                                Some(DanmakuEvent::Chat(mut item)) => {
                                    // Douyin replays recent history on connect; those
                                    // messages precede the recording start and would all
                                    // collapse onto offset 0. Skip them (5s clock skew
                                    // tolerance).
                                    let event_time = (item.timestamp_ms > 0)
                                        .then(|| DateTime::from_timestamp_millis(item.timestamp_ms))
                                        .flatten();
                                    if let Some(event) = event_time {
                                        if event
                                            < current_start - chrono::Duration::seconds(5)
                                        {
                                            continue;
                                        }
                                    }
                                    item.time_offset_secs =
                                        writer::time_offset_secs(current_start, event_time);
                                    if let Ok(slot) = task_writer.lock() {
                                        if let Some(writer) = slot.as_ref() {
                                            let _ = writer.write_item(&item);
                                        }
                                    }
                                    task_count.fetch_add(1, Ordering::Relaxed);
                                    task_total.fetch_add(1, Ordering::Relaxed);
                                    let _ = status_tx.send(DanmakuStatus {
                                        task_id,
                                        count: task_count.load(Ordering::Relaxed),
                                        total_count: task_total.load(Ordering::Relaxed),
                                        segment_index: Some(current_index),
                                        state: "recording", error: None,
                                    });
                                }
                                Some(DanmakuEvent::StreamClosed { tips }) => {
                                    let _ = status_tx.send(DanmakuStatus {
                                        task_id,
                                        count: task_count.load(Ordering::Relaxed),
                                        total_count: task_total.load(Ordering::Relaxed),
                                        segment_index: Some(current_index),
                                        state: "stream_closed",
                                        error: tips,
                                    });
                                }
                                Some(DanmakuEvent::Reconnecting { attempt, reason }) => {
                                    let _ = status_tx.send(DanmakuStatus {
                                        task_id,
                                        count: task_count.load(Ordering::Relaxed),
                                        total_count: task_total.load(Ordering::Relaxed),
                                        segment_index: Some(current_index),
                                        state: "reconnecting",
                                        error: Some(format!("第 {attempt} 次重连: {reason}")),
                                    });
                                }
                                None => {
                                    // WebSocket session ended without a stop signal.
                                    finalize(&task_writer);
                                    let _ = status_tx.send(DanmakuStatus {
                                        task_id,
                                        count: task_count.load(Ordering::Relaxed),
                                        total_count: task_total.load(Ordering::Relaxed),
                                        segment_index: Some(current_index),
                                        state: "ended", error: None,
                                    });
                                    return;
                                }
                            }
                        }
                    }
                }
            })
        };

        sessions.insert(
            config.task_id,
            Session {
                shutdown: shutdown_tx,
                commands: command_tx,
                task: processor,
                segment_index: shared_index,
                path: shared_path,
                count,
                total_count,
            },
        );
        Ok(())
    }

    /// Rotate the session to the next video segment: finalize the current XML
    /// and open a new one with timestamps relative to the segment start. The
    /// WebSocket stays connected, so no messages are lost across the boundary.
    /// Returns `true` only when an actual rotation happened (the segment index
    /// changed); `false` when no session exists or it already follows the
    /// requested segment.
    pub fn rotate(
        &self,
        task_id: i64,
        segment_index: u32,
        path: PathBuf,
        start_time: DateTime<Utc>,
    ) -> bool {
        let Ok(sessions) = self.sessions.lock() else {
            return false;
        };
        let Some(session) = sessions.get(&task_id) else {
            return false;
        };
        if session.segment_index.load(Ordering::Relaxed) == u64::from(segment_index) {
            return false;
        }
        session
            .commands
            .send(ProcessorCommand::Rotate {
                path,
                start_time,
                segment_index,
            })
            .is_ok()
    }

    /// Stop every active danmaku session (used on shutdown), finalizing each
    /// XML file and waiting for the processor tasks to finish.
    pub async fn shutdown_all(&self) {
        let sessions: Vec<(JoinHandle<()>, PathBuf, u64, u32, u64)> = {
            let Ok(mut sessions) = self.sessions.lock() else {
                return;
            };
            sessions
                .drain()
                .map(|(_, session)| {
                    let count = session.count.load(Ordering::Relaxed);
                    let total = session.total_count.load(Ordering::Relaxed);
                    let index = session.segment_index.load(Ordering::Relaxed) as u32;
                    let path = session.path.lock().map(|p| p.clone()).unwrap_or_default();
                    let _ = session.shutdown.send(true);
                    (session.task, path, count, index, total)
                })
                .collect()
        };
        for (task, _, _, _, _) in sessions {
            let _ = task.await;
        }
    }

    /// Stop the danmaku session for a task. Returns the task handle so the
    /// caller can await finalization, the final XML path, the current segment
    /// count, the current segment index, and the total count across rotations.
    pub fn stop(&self, task_id: i64) -> Option<(JoinHandle<()>, PathBuf, u64, u32, u64)> {
        let mut sessions = self.sessions.lock().ok()?;
        let session = sessions.remove(&task_id)?;
        let count = session.count.load(Ordering::Relaxed);
        let total = session.total_count.load(Ordering::Relaxed);
        let index = session.segment_index.load(Ordering::Relaxed) as u32;
        let path = session.path.lock().ok()?.clone();
        let _ = session.shutdown.send(true);
        Some((session.task, path, count, index, total))
    }
}

fn finalize(writer: &std::sync::Mutex<Option<SharedDanmakuWriter>>) {
    if let Ok(mut slot) = writer.lock() {
        if let Some(writer) = slot.take() {
            let _ = writer.finalize();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn manager_tracks_sessions() {
        let manager = DanmakuManager::new();
        let (tx, _rx) = mpsc::unbounded_channel();
        let dir = tempfile::tempdir().unwrap();
        manager
            .start(
                DanmakuConfig {
                    task_id: 1,
                    room_id: "123".into(),
                    ttwid: "ttwid".into(),
                    cookie: String::new(),
                    output_path: dir.path().join("part001.xml"),
                    start_time: Utc::now(),
                    segment_index: 1,
                },
                tx,
            )
            .unwrap();
        let (handle, path, _count, _index, _total) = manager.stop(1).expect("session exists");
        assert_eq!(path, dir.path().join("part001.xml"));
        // The processor exits as soon as shutdown flips.
        handle.await.unwrap();
        assert!(manager.stop(1).is_none());
    }

    #[tokio::test]
    async fn rotate_switches_output_without_dropping_session() {
        let manager = DanmakuManager::new();
        let (tx, mut rx) = mpsc::unbounded_channel();
        let dir = tempfile::tempdir().unwrap();
        manager
            .start(
                DanmakuConfig {
                    task_id: 2,
                    room_id: "123".into(),
                    ttwid: "ttwid".into(),
                    cookie: String::new(),
                    output_path: dir.path().join("part001.xml"),
                    start_time: Utc::now(),
                    segment_index: 1,
                },
                tx,
            )
            .unwrap();
        let rotated = manager.rotate(2, 2, dir.path().join("part002.xml"), Utc::now());
        assert!(rotated);
        // The rotate command arrives before the stop, so the final path is
        // the second segment's.
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(2);
        let mut rotated_seen = false;
        while tokio::time::Instant::now() < deadline && !rotated_seen {
            if let Ok(status) = rx.try_recv() {
                if status.state == "rotated" && status.segment_index == Some(2) {
                    rotated_seen = true;
                }
            } else {
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
        }
        assert!(rotated_seen, "rotation status not observed");
        let (handle, path, _count, index, _total) = manager.stop(2).expect("session exists");
        assert_eq!(index, 2);
        assert_eq!(path, dir.path().join("part002.xml"));
        handle.await.unwrap();
    }

    #[tokio::test]
    async fn rotate_is_idempotent_for_same_index() {
        let manager = DanmakuManager::new();
        let (tx, _rx) = mpsc::unbounded_channel();
        let dir = tempfile::tempdir().unwrap();
        manager
            .start(
                DanmakuConfig {
                    task_id: 3,
                    room_id: "123".into(),
                    ttwid: "ttwid".into(),
                    cookie: String::new(),
                    output_path: dir.path().join("part001.xml"),
                    start_time: Utc::now(),
                    segment_index: 1,
                },
                tx,
            )
            .unwrap();
        assert!(!manager.rotate(3, 1, dir.path().join("x.xml"), Utc::now()));
        let (handle, path, _count, index, _total) = manager.stop(3).expect("session exists");
        assert_eq!(index, 1);
        assert_eq!(path, dir.path().join("part001.xml"));
        handle.await.unwrap();
    }
}
