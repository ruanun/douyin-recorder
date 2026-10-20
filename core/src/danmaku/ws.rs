//! Douyin webcast WebSocket client.
//!
//! Protocol reference:
//! - <https://github.com/saermart/DouyinLiveWebFetcher> (original fetcher, douyin.proto)
//! - <https://github.com/hua0512/rust-srec> (Rust implementation, Apache-2.0)
//!
//! Flow: connect to `wss://webcast100-ws-web-lf.douyin.com/webcast/im/push/v2/`
//! with room and browser parameters plus an X-Bogus signature. Every binary
//! frame is a protobuf `PushFrame` whose payload is gzip-compressed
//! `Response`; chat messages arrive as `WebcastChatMessage` payloads.

use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use prost::Message as ProstMessage;
use rand::RngExt;
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::HeaderValue;
use tokio_tungstenite::tungstenite::Message as WsMessage;

use super::proto;
use super::signature::{generate_xbogus, md5_hex};
use super::writer::DanmakuItem;

/// rustls needs an installed crypto provider before the first connection.
/// `tokio-tungstenite` does not enable one by itself.
static RUSTLS_INIT: std::sync::Once = std::sync::Once::new();

/// Install the ring crypto provider for rustls (idempotent). Call before
/// establishing WebSocket connections in custom binaries/tests.
pub fn ensure_rustls() {
    RUSTLS_INIT.call_once(|| {
        let _ = rustls::crypto::ring::default_provider().install_default();
    });
}

/// Douyin webcast WebSocket server hosts (rotated randomly).
const WS_HOSTS: &[&str] = &[
    "wss://webcast100-ws-web-lq.douyin.com",
    "wss://webcast100-ws-web-hl.douyin.com",
    "wss://webcast100-ws-web-lf.douyin.com",
];

const WS_PATH: &str = "/webcast/im/push/v2/";
const VERSION_CODE: &str = "180800";
const WEBCAST_SDK_VERSION: &str = "1.0.15";
const UPDATE_VERSION_CODE: &str = "1.0.15";
const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(10);
const RECONNECT_DELAY: Duration = Duration::from_secs(3);
const MAX_RECONNECT_DELAY: Duration = Duration::from_secs(60);
/// Reconnect when the server stays silent for this long even though heartbeats
/// are being sent (detects half-open connections that never report a close).
const LIVENESS_TIMEOUT: Duration = Duration::from_secs(75);

pub const DOUYIN_UA: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36";

/// Events emitted by the WebSocket session.
#[derive(Debug, Clone)]
pub enum DanmakuEvent {
    Connected,
    Chat(DanmakuItem),
    /// `WebcastControlMessage` action 3: the stream has ended.
    StreamClosed {
        tips: Option<String>,
    },
    /// The connection dropped and a reconnect attempt is scheduled.
    Reconnecting {
        attempt: u64,
        reason: String,
    },
}

/// Connection parameters for one room.
pub struct WsSession {
    room_id: String,
    cookie_header: String,
}

impl WsSession {
    pub fn new(room_id: impl Into<String>, ttwid: &str, user_cookie: &str) -> Self {
        let mut cookies = vec![format!("ttwid={ttwid}")];
        for part in user_cookie
            .split(';')
            .map(str::trim)
            .filter(|p| !p.is_empty())
        {
            let is_ttwid = part
                .split_once('=')
                .map(|(name, _)| name.trim().eq_ignore_ascii_case("ttwid"))
                .unwrap_or(true);
            if !is_ttwid {
                cookies.push(part.to_string());
            }
        }
        Self {
            room_id: room_id.into(),
            cookie_header: cookies.join("; "),
        }
    }

    fn generate_user_unique_id() -> String {
        use std::time::{SystemTime, UNIX_EPOCH};
        let base = 7_300_000_000_000_000_000u64;
        let range = 699_999_999_999_999_999u64;
        let ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0);
        (base + (ts % range)).to_string()
    }

    /// Build the signed WebSocket URL, following the browser client's
    /// parameter layout and X-Bogus signature scheme.
    pub fn websocket_url(&self) -> String {
        let user_unique_id = Self::generate_user_unique_id();

        let signature_param = format!(
            "live_id=1,aid=6383,version_code={VERSION_CODE},webcast_sdk_version={WEBCAST_SDK_VERSION},room_id={},sub_room_id=,sub_channel_id=,did_rule=3,user_unique_id={},device_platform=web,device_type=,ac=,identity=audience",
            self.room_id, user_unique_id
        );
        let signature = generate_xbogus(&md5_hex(&signature_param), 1);

        let query = format!(
            "app_name=douyin_web&compress=gzip&device_platform=web&browser_language=zh-CN&browser_platform=Win32&browser_name=Mozilla&browser_version=120.0.0.0&aid=6383&live_id=1&enter_from=web_live&version_code={VERSION_CODE}&webcast_sdk_version={WEBCAST_SDK_VERSION}&update_version_code={UPDATE_VERSION_CODE}&host=https://live.douyin.com&did_rule=3&identity=audience&endpoint=live_pc&need_persist_msg_count=15&heartbeatDuration=0&room_id={}&user_unique_id={}&signature={}",
            self.room_id, user_unique_id, signature
        );

        let host = WS_HOSTS[rand::rng().random_range(0..WS_HOSTS.len())];
        format!("{host}{WS_PATH}?{query}")
    }

    fn request(&self) -> Result<impl IntoClientRequest, String> {
        let mut request = self
            .websocket_url()
            .into_client_request()
            .map_err(|e| format!("构建弹幕 WebSocket 请求失败: {e}"))?;
        let headers = request.headers_mut();
        headers.insert(
            "Origin",
            HeaderValue::from_static("https://live.douyin.com"),
        );
        headers.insert("User-Agent", HeaderValue::from_static(DOUYIN_UA));
        headers.insert(
            "Cookie",
            HeaderValue::from_str(&self.cookie_header)
                .map_err(|_| "弹幕 cookie 包含非法字符".to_string())?,
        );
        Ok(request)
    }
}

/// Runs one WebSocket session until stopped or failed, emitting events through
/// `tx`. Returns the stop reason. Reconnects with exponential backoff while
/// the `shutdown` channel is open.
pub async fn run_session(
    session: WsSession,
    tx: mpsc::UnboundedSender<DanmakuEvent>,
    mut shutdown: tokio::sync::watch::Receiver<bool>,
) -> Result<(), String> {
    let mut attempt: u64 = 0;
    let mut delay = RECONNECT_DELAY;

    loop {
        if *shutdown.borrow() {
            return Ok(());
        }
        attempt += 1;
        let result = run_connection(&session, &tx, &mut shutdown).await;
        if *shutdown.borrow() {
            return Ok(());
        }
        match result {
            Ok(()) => {
                // The stream ended cleanly (or the server closed).
                return Ok(());
            }
            Err(error) => {
                let _ = tx.send(DanmakuEvent::Reconnecting {
                    attempt,
                    reason: error.clone(),
                });
                tokio::select! {
                    _ = shutdown.changed() => return Ok(()),
                    _ = tokio::time::sleep(delay) => {}
                }
                delay = (delay * 2).min(MAX_RECONNECT_DELAY);
            }
        }
    }
}

async fn run_connection(
    session: &WsSession,
    tx: &mpsc::UnboundedSender<DanmakuEvent>,
    shutdown: &mut tokio::sync::watch::Receiver<bool>,
) -> Result<(), String> {
    ensure_rustls();
    let request = session.request()?;
    let (mut websocket, _response) = tokio_tungstenite::connect_async(request)
        .await
        .map_err(|e| format!("连接弹幕服务器失败: {e}"))?;
    let _ = tx.send(DanmakuEvent::Connected);

    let mut heartbeat = tokio::time::interval(HEARTBEAT_INTERVAL);
    heartbeat.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    // Consume the immediate first tick.
    heartbeat.tick().await;
    let mut last_frame = tokio::time::Instant::now();

    loop {
        if last_frame.elapsed() >= LIVENESS_TIMEOUT {
            return Err("弹幕连接长时间无数据，主动重连".to_string());
        }
        tokio::select! {
            _ = shutdown.changed() => {
                if *shutdown.borrow() {
                    let _ = websocket.close(None).await;
                    return Ok(());
                }
            }
            _ = heartbeat.tick() => {
                let frame = proto::PushFrame {
                    payload_type: "hb".to_string(),
                    ..Default::default()
                };
                let mut buf = Vec::new();
                frame.encode(&mut buf).map_err(|e| format!("编码心跳失败: {e}"))?;
                websocket
                    .send(WsMessage::Binary(buf.into()))
                    .await
                    .map_err(|e| format!("发送心跳失败: {e}"))?;
            }
            incoming = websocket.next() => {
                last_frame = tokio::time::Instant::now();
                match incoming {
                    Some(Ok(WsMessage::Binary(data))) => {
                        handle_binary(&mut websocket, &data, tx).await?;
                    }
                    Some(Ok(WsMessage::Close(_))) | None => {
                        return Err("弹幕连接已关闭".to_string());
                    }
                    Some(Ok(WsMessage::Ping(payload))) => {
                        let _ = websocket.send(WsMessage::Pong(payload)).await;
                    }
                    Some(Ok(_)) => {}
                    Some(Err(e)) => {
                        return Err(format!("弹幕连接错误: {e}"));
                    }
                }
            }
        }
    }
}

async fn handle_binary<S>(
    websocket: &mut S,
    data: &[u8],
    tx: &mpsc::UnboundedSender<DanmakuEvent>,
) -> Result<(), String>
where
    S: SinkExt<WsMessage, Error = tokio_tungstenite::tungstenite::Error> + Unpin,
{
    let frame = proto::PushFrame::decode(data).map_err(|e| format!("解析弹幕帧失败: {e}"))?;

    let compress_type = frame
        .headers
        .iter()
        .find(|h| h.key == "compress_type")
        .map(|h| h.value.as_str())
        .unwrap_or("gzip");

    let payload = if compress_type == "gzip" {
        use std::io::Read;
        let mut decoder = flate2::read::GzDecoder::new(&frame.payload[..]);
        let mut decompressed = Vec::new();
        decoder
            .read_to_end(&mut decompressed)
            .map_err(|e| format!("解压弹幕数据失败: {e}"))?;
        decompressed
    } else {
        frame.payload.clone()
    };

    let response = proto::Response::decode(payload.as_slice())
        .map_err(|e| format!("解析弹幕响应失败: {e}"))?;

    // Acknowledge when requested, mirroring the internal extension back.
    if response.need_ack {
        let ack = proto::PushFrame {
            log_id: frame.log_id,
            payload_type: "ack".to_string(),
            payload: response.internal_ext.clone().into_bytes(),
            ..Default::default()
        };
        let mut buf = Vec::new();
        ack.encode(&mut buf)
            .map_err(|e| format!("编码 ack 失败: {e}"))?;
        websocket
            .send(WsMessage::Binary(buf.into()))
            .await
            .map_err(|e| format!("发送 ack 失败: {e}"))?;
    }

    for message in &response.messages {
        match message.method.as_str() {
            "WebcastChatMessage" => {
                if let Some(item) = parse_chat_message(&message.payload) {
                    let _ = tx.send(DanmakuEvent::Chat(item));
                }
            }
            "WebcastControlMessage" => {
                if let Ok(control) = proto::ControlMessage::decode(message.payload.as_slice()) {
                    if control.action == 3 {
                        let tips = (!control.tips.trim().is_empty()).then_some(control.tips);
                        let _ = tx.send(DanmakuEvent::StreamClosed { tips });
                    }
                }
            }
            _ => {}
        }
    }
    Ok(())
}

/// Parse a `WebcastChatMessage` payload into a danmaku item.
fn parse_chat_message(payload: &[u8]) -> Option<DanmakuItem> {
    let chat = proto::ChatMessage::decode(payload).ok()?;
    let user = chat.user.as_ref();

    // Preferred event timestamp: event_time, then common.create_time.
    // Douyin mixes second- and millisecond-precision values here, so normalize:
    // anything below 1e12 is a seconds timestamp (1e12 ms is year 2001).
    // Zero means the timestamp is unknown and the caller falls back to now.
    let raw = if chat.event_time != 0 {
        chat.event_time as i64
    } else {
        chat.common.as_ref().map(|c| c.create_time).unwrap_or(0) as i64
    };
    let timestamp_ms = normalize_timestamp_ms(raw);

    let color = chat
        .rtf_content_v2
        .as_ref()
        .and_then(|t| t.default_format.as_ref().map(|f| f.color.clone()))
        .or_else(|| {
            chat.rtf_content
                .as_ref()
                .and_then(|t| t.default_format.as_ref().map(|f| f.color.clone()))
        })
        .filter(|c| !c.is_empty())
        .unwrap_or_else(|| "#FFFFFF".to_string());

    let color_decimal = parse_hex_color(&color).unwrap_or(16777215);

    Some(DanmakuItem {
        time_offset_secs: 0.0, // filled in by the session manager
        mode: 1,
        font_size: 25,
        color: color_decimal,
        timestamp_ms,
        user_id: user.map(|u| u.id).unwrap_or(0),
        nickname: user.map(|u| u.nickname.clone()).unwrap_or_default(),
        content: chat.content,
    })
}

/// Normalize a Douyin message timestamp to milliseconds. Douyin mixes
/// second- and millisecond-precision values; values below 1e12 are seconds.
pub fn normalize_timestamp_ms(raw: i64) -> i64 {
    if raw > 0 && raw < 1_000_000_000_000 {
        raw * 1000
    } else {
        raw
    }
}

/// Parse "#RRGGBB" / "RRGGBB" into a decimal RGB value.
pub fn parse_hex_color(input: &str) -> Option<u32> {
    let hex = input.trim().trim_start_matches('#');
    if hex.len() != 6 {
        return None;
    }
    u32::from_str_radix(hex, 16).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::danmaku::writer::DanmakuItem;

    #[test]
    fn websocket_url_contains_room_and_signature() {
        let session = WsSession::new("123456", "ttwid_value", "");
        let url = session.websocket_url();
        assert!(url.starts_with("wss://"));
        assert!(url.contains("/webcast/im/push/v2/"));
        assert!(url.contains("room_id=123456"));
        assert!(url.contains("&signature="));
        let signature = url.split("signature=").nth(1).unwrap();
        assert_eq!(signature.len(), 16);
    }

    #[test]
    fn timestamps_normalize_from_seconds_and_milliseconds() {
        assert_eq!(normalize_timestamp_ms(1_792_217_910), 1_792_217_910_000);
        assert_eq!(normalize_timestamp_ms(1_792_217_910_123), 1_792_217_910_123);
        assert_eq!(normalize_timestamp_ms(0), 0);
    }

    #[test]
    fn hex_color_parsing() {
        assert_eq!(parse_hex_color("#FFFFFF"), Some(16777215));
        assert_eq!(parse_hex_color("FF0000"), Some(16711680));
        assert_eq!(parse_hex_color("#FF00FF"), Some(16711935));
        assert_eq!(parse_hex_color("invalid"), None);
    }

    #[test]
    fn chat_message_parses_from_protobuf() {
        let user = proto::User {
            id: 42,
            nickname: "观众".to_string(),
            ..Default::default()
        };
        let chat = proto::ChatMessage {
            user: Some(user),
            content: "你好".to_string(),
            event_time: 1_700_000_000_000,
            ..Default::default()
        };
        let mut payload = Vec::new();
        chat.encode(&mut payload).unwrap();
        let item: DanmakuItem = parse_chat_message(&payload).expect("parses");
        assert_eq!(item.user_id, 42);
        assert_eq!(item.nickname, "观众");
        assert_eq!(item.content, "你好");
        assert_eq!(item.timestamp_ms, 1_700_000_000_000);
        assert_eq!(item.color, 16777215);
    }
}
